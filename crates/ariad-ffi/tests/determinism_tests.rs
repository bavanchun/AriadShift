//! Every Free tool in `schemas/tools.v1.json` must produce identical bytes through the UniFFI
//! `run_tool` and through the `ashift` CLI for the same inputs and parameters.
//!
//! `tests/fixtures/tool-params.json` holds the minimal valid inputs and params per tool id, plus an
//! explicit allow-list of Free tools that cannot run here (engine pack or network needed). The
//! test fails loudly when a Free tool is in neither, so a new Free tool cannot be added silently.
//! Both surfaces run with the same `SOURCE_DATE_EPOCH`, the repository's reproducibility switch.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use ariad_ffi::{ToolRunRequest, list_tools, run_tool, set_engine_program};
use serde_json::Value;

const EPOCH: &str = "1700000000";
const EPOCH_VAR: &str = "SOURCE_DATE_EPOCH";
const PLAN_VAR: &str = "ASHIFT_PLAN";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// Locates `ashift` next to the test binary.
///
/// Under nextest the workspace build already produced it with the workspace feature set, and a
/// second build here would replace it, so it is only built for a plain `cargo test -p ariad-ffi`.
fn ashift_binary() -> PathBuf {
    if std::env::var_os("NEXTEST").is_none() {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
        let status = Command::new(cargo)
            .args(["build", "-p", "ariad-cli", "--bin", "ashift", "--quiet"])
            .current_dir(repo_root())
            .status()
            .expect("spawn cargo build for ashift");
        assert!(
            status.success(),
            "cargo build -p ariad-cli --bin ashift failed"
        );
    }
    let exe = std::env::current_exe().expect("test exe path");
    let profile_dir = exe
        .parent()
        .and_then(Path::parent)
        .expect("target profile directory");
    let binary = profile_dir.join(if cfg!(windows) {
        "ashift.exe"
    } else {
        "ashift"
    });
    assert!(binary.is_file(), "ashift not found at {}", binary.display());
    binary
}

/// Every file under `dir` keyed by its path relative to `dir`.
fn read_tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).expect("read output dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path.strip_prefix(root).unwrap().display().to_string();
                out.insert(rel, fs::read(&path).expect("read output"));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

struct Case {
    inputs: Vec<PathBuf>,
    params: Value,
}

fn case_for(id: &str, entry: &Value) -> Case {
    let inputs = entry["inputs"]
        .as_array()
        .unwrap_or_else(|| panic!("{id}: `inputs` must be an array"))
        .iter()
        .map(|v| repo_root().join(v.as_str().expect("input path string")))
        .collect();
    Case {
        inputs,
        params: entry
            .get("params")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    }
}

type Tree = BTreeMap<String, Vec<u8>>;

/// Runs the tool through the CLI into a fresh directory and returns the files it published.
fn run_cli(ashift: &Path, id: &str, case: &Case, scratch: &Path) -> Result<Tree, String> {
    let out = scratch.join("cli-out");
    fs::create_dir_all(&out).unwrap();
    let params_file = scratch.join("cli-params.json");
    fs::write(&params_file, serde_json::to_vec(&case.params).unwrap()).unwrap();
    let output = Command::new(ashift)
        .arg("tool")
        .arg(id)
        .args(&case.inputs)
        .arg("-o")
        .arg(&out)
        .arg("--params-json")
        .arg(&params_file)
        .env(EPOCH_VAR, EPOCH)
        .env_remove(PLAN_VAR)
        .output()
        .expect("spawn ashift");
    if !output.status.success() {
        return Err(format!(
            "cli exit {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(read_tree(&out))
}

async fn run_ffi(id: &str, case: &Case, scratch: &Path) -> Result<Tree, String> {
    let out = scratch.join("ffi-out");
    fs::create_dir_all(&out).unwrap();
    let request = ToolRunRequest {
        tool: id.to_string(),
        inputs: case
            .inputs
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
        output_dir: Some(out.display().to_string()),
        output_path: None,
        params_json: Some(case.params.to_string()),
        options_json: None,
        cancel_token: None,
        secrets: None,
        plan: None,
    };
    run_tool(request, None).await.map_err(|e| e.to_string())?;
    Ok(read_tree(&out))
}

/// Output files must match by name and by exact bytes. Nothing is normalised: with the shared
/// `SOURCE_DATE_EPOCH` both surfaces already publish identical files.
fn compare_trees(ffi: &Tree, cli: &Tree) -> Result<(), String> {
    if ffi.is_empty() {
        return Err("produced no output files".to_string());
    }
    if ffi.keys().ne(cli.keys()) {
        return Err(format!(
            "file sets differ: ffi {:?} vs cli {:?}",
            ffi.keys().collect::<Vec<_>>(),
            cli.keys().collect::<Vec<_>>()
        ));
    }
    match ffi.iter().find(|(name, bytes)| **bytes != cli[*name]) {
        Some((name, _)) => Err(format!("bytes differ in {name}")),
        None => Ok(()),
    }
}

#[tokio::test]
async fn every_free_tool_matches_cli_output() {
    let root = repo_root();
    let schema = read_json(&root.join("schemas/tools.v1.json"));
    let table = read_json(&root.join("crates/ariad-ffi/tests/fixtures/tool-params.json"));

    // Free tools in the schema, cross-checked against the registry the app sees.
    let schema_tools = schema["tools"].as_array().expect("schema tools array");
    let free: BTreeMap<String, &Value> = schema_tools
        .iter()
        .filter(|t| t["tier"] == "free")
        .map(|t| (t["id"].as_str().unwrap().to_string(), t))
        .collect();
    let registry_free: BTreeSet<String> = list_tools()
        .expect("list_tools")
        .into_iter()
        .filter(|t| t.tier == "free")
        .map(|t| t.id)
        .collect();
    assert_eq!(
        free.keys().cloned().collect::<BTreeSet<_>>(),
        registry_free,
        "schemas/tools.v1.json and the registry disagree on the Free tool set"
    );

    let cases = table["tools"].as_object().expect("`tools` table");
    let allow = table["allow_list"].as_object().expect("`allow_list` table");

    // Structure: every Free tool is covered exactly once, nothing stale, every reason stated.
    let missing: Vec<_> = free
        .keys()
        .filter(|id| !cases.contains_key(*id) && !allow.contains_key(*id))
        .collect();
    assert!(
        missing.is_empty(),
        "Free tools missing from tests/fixtures/tool-params.json (add params or an allow-list reason): {missing:?}"
    );
    let overlap: Vec<_> = cases.keys().filter(|id| allow.contains_key(*id)).collect();
    assert!(
        overlap.is_empty(),
        "tools in both `tools` and `allow_list`: {overlap:?}"
    );
    let unknown: Vec<_> = cases
        .keys()
        .chain(allow.keys())
        .filter(|id| !free.contains_key(*id))
        .collect();
    assert!(
        unknown.is_empty(),
        "entries that are not Free registry tools: {unknown:?}"
    );
    for (id, entry) in allow {
        let requires = entry["requires"].as_str().unwrap_or_default();
        assert!(
            requires == "network" || requires.starts_with("pack:"),
            "allow-list entry {id}: `requires` must be `network` or `pack:<name>`, got {requires:?}"
        );
        assert!(
            entry["reason"]
                .as_str()
                .is_some_and(|r| r.trim().len() >= 8),
            "allow-list entry {id} needs a reason"
        );
        // In-process tools have no excuse: only tools that run an engine pack or the network may be listed.
        let engine = free[id]["engine"].as_str().unwrap_or_default();
        assert!(
            engine != "ariad-pdf" && engine != "ariad-core",
            "allow-list entry {id}: engine `{engine}` runs in-process here and must be in `tools`"
        );
        if let Some(pack) = requires.strip_prefix("pack:")
            && engine != "ariad-host"
        {
            assert_eq!(
                pack, engine,
                "allow-list entry {id}: pack differs from its engine"
            );
        }
    }

    // Params must satisfy each tool's schema.
    let mut invalid = Vec::new();
    for (id, entry) in cases {
        let validator = jsonschema::validator_for(&free[id]["params_schema"])
            .unwrap_or_else(|e| panic!("{id}: params schema does not compile: {e}"));
        let params = entry
            .get("params")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        let errors: Vec<_> = validator
            .iter_errors(&params)
            .map(|e| e.to_string())
            .collect();
        if !errors.is_empty() {
            invalid.push(format!("{id}: {}", errors.join("; ")));
        }
    }
    assert!(
        invalid.is_empty(),
        "params violate their tool schema:\n{}",
        invalid.join("\n")
    );

    // Same reproducibility clock for both surfaces.
    // SAFETY: this is the only test in the binary, so no other thread reads the environment.
    unsafe { std::env::set_var(EPOCH_VAR, EPOCH) };
    let ashift = ashift_binary();
    set_engine_program(ashift.display().to_string()).expect("set engine program");

    let mut failures = Vec::new();
    let mut compared = 0_usize;
    for (id, entry) in cases {
        let case = case_for(id, entry);
        let scratch = tempfile::tempdir().expect("scratch dir");
        let ffi = run_ffi(id, &case, scratch.path()).await;
        let cli = run_cli(&ashift, id, &case, scratch.path());
        match (ffi, cli) {
            (Ok(a), Ok(b)) => match compare_trees(&a, &b) {
                Ok(()) => compared += 1,
                Err(why) => failures.push(format!("{id}: {why}")),
            },
            (a, b) => failures.push(format!(
                "{id}: did not run on both surfaces (ffi: {}, cli: {})",
                a.err().unwrap_or_else(|| "ok".into()),
                b.err().unwrap_or_else(|| "ok".into())
            )),
        }
    }

    // Allow-listed tools: a probe proves each one really is unavailable here, on both surfaces.
    // When the pack happens to be installed, the probe is compared like any other tool.
    let (mut probed_unavailable, mut probed_compared, mut unprobed) = (0_usize, 0_usize, 0_usize);
    for (id, entry) in allow {
        let Some(probe) = entry.get("probe") else {
            unprobed += 1;
            continue;
        };
        let case = case_for(id, probe);
        let scratch = tempfile::tempdir().expect("scratch dir");
        let ffi = run_ffi(id, &case, scratch.path()).await;
        let cli = run_cli(&ashift, id, &case, scratch.path());
        match (ffi, cli) {
            (Err(a), Err(b)) if a.contains("tool_missing") && b.contains("tool_missing") => {
                probed_unavailable += 1;
            }
            (Ok(a), Ok(b)) => match compare_trees(&a, &b) {
                Ok(()) => probed_compared += 1,
                Err(why) => failures.push(format!("{id}: {why}")),
            },
            (a, b) => failures.push(format!(
                "{id}: allow-listed as `{}` but the probe did not fail with tool_missing on both surfaces (ffi: {}, cli: {})",
                entry["requires"].as_str().unwrap_or_default(),
                a.err().unwrap_or_else(|| "ok".into()),
                b.err().unwrap_or_else(|| "ok".into())
            )),
        }
    }
    println!(
        "determinism: {} Free tools: {compared} compared byte-for-byte; {} allow-listed \
         ({probed_unavailable} probed unavailable, {probed_compared} probed and compared, {unprobed} without a probe)",
        free.len(),
        allow.len()
    );
    assert!(
        failures.is_empty(),
        "determinism failures:\n{}",
        failures.join("\n")
    );
}

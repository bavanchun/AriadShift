//! End-to-end tests for `ashift engines install|remove|verify`, the engine and doctor listings
//! of installed packs, MCP `list_engines`, and `ashift __pack manifest`.

use std::{
    fs,
    io::{BufRead, BufReader, Cursor, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use serde_json::{Value, json};
use tempfile::TempDir;

fn engine_probe_path() -> PathBuf {
    Path::new(env!("CARGO_BIN_EXE_ashift"))
        .with_file_name(format!("engine-probe{}", std::env::consts::EXE_SUFFIX))
}

fn probe_file_name() -> String {
    format!("engine-probe{}", std::env::consts::EXE_SUFFIX)
}

/// A signed pack in `source`, an empty data directory, and the trusted public key file.
struct Fixture {
    source: TempDir,
    data: TempDir,
    key_file: PathBuf,
    _keys: TempDir,
    pair: minisign::KeyPair,
}

impl Fixture {
    fn new() -> Self {
        let keys = TempDir::new().unwrap();
        let pair = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let key_file = keys.path().join("test.pub");
        fs::write(&key_file, pair.pk.to_box().unwrap().into_string()).unwrap();
        let fixture = Self {
            source: TempDir::new().unwrap(),
            data: TempDir::new().unwrap(),
            key_file,
            _keys: keys,
            pair,
        };
        fixture.build_pack();
        fixture
    }

    /// Builds the pack with `ashift __pack manifest --base`, then signs it.
    fn build_pack(&self) {
        let dir = self.source.path();
        let program = probe_file_name();
        fs::copy(engine_probe_path(), dir.join(&program)).unwrap();
        fs::write(dir.join("LICENSE.txt"), "MIT licence\n").unwrap();
        let triple = ariad_host::packs::host_targets()[0].clone();
        let template = dir.join("template.toml");
        fs::write(
            &template,
            format!(
                r#"schema = "ariad-pack/1"
name = "probe"
version = "0.0.0"
description = "Protocol probe used by tests"
license = "MIT"
requires_ashift = ">=0.1.0"
notices = ["LICENSE.txt"]

[[targets]]
triple = "{triple}"
entrypoint = ["bin/{program}", "describe-no-memory-limit"]
env = {{ HF_HUB_OFFLINE = "1" }}
"#
            ),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ashift"))
            .args([
                "__pack",
                "manifest",
                "--name",
                "probe",
                "--version",
                "1.0.0",
            ])
            .args(["--target", &triple])
            .arg("--base")
            .arg(&template)
            .args(["--dest", &format!("{program}=bin")])
            .arg("-o")
            .arg(dir.join("pack.toml"))
            .arg(format!("{program}={}", dir.join(&program).display()))
            .arg(format!("LICENSE.txt={}", dir.join("LICENSE.txt").display()))
            .output()
            .expect("run __pack manifest");
        assert!(
            output.status.success(),
            "__pack manifest failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        fs::remove_file(&template).unwrap();
        self.sign("pack:probe version:1.0.0");
    }

    fn sign(&self, trusted_comment: &str) {
        let manifest = fs::read(self.source.path().join("pack.toml")).unwrap();
        let signature = minisign::sign(
            Some(&self.pair.pk),
            &self.pair.sk,
            Cursor::new(manifest),
            Some(trusted_comment),
            None,
        )
        .unwrap()
        .into_string();
        fs::write(self.source.path().join("pack.toml.minisig"), signature).unwrap();
    }

    fn ashift(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ashift"));
        command
            .env("ASHIFT_DATA_DIR", self.data.path())
            .env("ASHIFT_TEST_PACK_KEYS", &self.key_file);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("ASHIFT_PACK_") {
                command.env_remove(key);
            }
        }
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.ashift().args(args).output().expect("run ashift")
    }

    fn install(&self) -> Output {
        self.ashift()
            .args(["engines", "install", "probe", "--json", "--from"])
            .arg(self.source.path())
            .output()
            .expect("run install")
    }
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not JSON ({error}): {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn pack_lifecycle_through_the_cli() {
    let fixture = Fixture::new();
    let data_dir = fixture.data.path().display().to_string();

    let output = fixture.install();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let report = json(&output);
    assert_eq!(report["name"], "probe");
    assert_eq!(report["version"], "1.0.0");
    assert_eq!(report["status"], "installed");
    assert_eq!(report["files"], 2);
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains(&data_dir),
        "JSON output carries no absolute paths"
    );

    let output = fixture
        .ashift()
        .args(["engines", "install", "probe", "--from"])
        .arg(fixture.source.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let human = String::from_utf8_lossy(&output.stdout);
    assert!(human.contains("probe 1.0.0"), "{human}");
    assert!(human.contains("is already installed in"), "{human}");

    let output = fixture.run(&["engines", "--json"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let rows = json(&output);
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "probe")
        .expect("the installed pack is listed");
    assert_eq!(row["status"], "ready", "{row}");
    assert_eq!(row["version"], "1.0.0");
    assert_eq!(row["license"], "MIT");
    assert_eq!(row["pack"]["source"], "installed");
    assert_eq!(row["pack"]["versions"], json!(["1.0.0"]));

    let output = fixture.run(&["engines"]);
    let human = String::from_utf8_lossy(&output.stdout);
    assert!(human.contains("PACK"), "{human}");
    assert!(
        human
            .lines()
            .any(|line| line.starts_with("probe") && line.contains("active")),
        "{human}"
    );

    let output = fixture.run(&["doctor", "--json"]);
    let doctor = json(&output);
    let checks = doctor["checks"].as_array().unwrap();
    let check = |id: &str| {
        checks
            .iter()
            .find(|check| check["id"] == id)
            .unwrap_or_else(|| panic!("doctor check {id} missing: {doctor}"))
            .clone()
    };
    assert_eq!(check("pack_probe")["ok"], true);
    assert_eq!(check("pack_keys")["ok"], true);
    assert_eq!(check("data_dir")["ok"], true);
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&data_dir));

    let output = fixture.run(&["engines", "verify", "probe"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));

    let installed_license = fixture
        .data
        .path()
        .join("packs")
        .join("probe")
        .join("1.0.0")
        .join("LICENSE.txt");
    fs::write(&installed_license, "changed\n").unwrap();
    let output = fixture.run(&["engines", "verify", "probe", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let report = json(&output);
    assert_eq!(report["ok"], false);
    assert_eq!(report["modified"], json!(["LICENSE.txt"]));
    assert!(stderr(&output).contains("failed verification"));

    let output = fixture.run(&["engines", "remove", "probe", "--json"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(json(&output)["removed"], json!(["1.0.0"]));
    let output = fixture.run(&["engines", "verify", "probe"]);
    assert_eq!(output.status.code(), Some(3));
    assert!(stderr(&output).contains("is not installed"));
}

#[test]
fn unknown_pack_exits_3_and_lists_known_packs() {
    let fixture = Fixture::new();
    let output = fixture.run(&["engines", "install", "no-such-pack"]);
    assert_eq!(output.status.code(), Some(3));
    assert!(
        stderr(&output).contains("known packs: docling"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn tampered_manifest_exits_1_with_a_signature_error() {
    let fixture = Fixture::new();
    let manifest = fixture.source.path().join("pack.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(&manifest, text.replace("Protocol probe", "Tampered probe")).unwrap();
    let output = fixture.install();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("pack signature verification failed"),
        "{}",
        stderr(&output)
    );
    assert!(!fixture.data.path().join("packs").join("probe").exists());
}

#[test]
fn builds_without_trusted_keys_refuse_packs() {
    let fixture = Fixture::new();
    let output = fixture
        .ashift()
        .env_remove("ASHIFT_TEST_PACK_KEYS")
        .args(["engines", "install", "probe", "--from"])
        .arg(fixture.source.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("no trusted pack signing keys"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn signature_for_another_version_exits_1() {
    let fixture = Fixture::new();
    fixture.sign("pack:probe version:0.9.0");
    let output = fixture.install();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("trusted comment"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn pack_manifest_needs_an_entrypoint() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("engine");
    fs::write(&file, b"x").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .args(["__pack", "manifest", "--name", "x", "--version", "1.0.0"])
        .args(["--target", "x86_64-unknown-linux-gnu"])
        .arg(&file)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("--entrypoint"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn mcp_list_engines_includes_installed_packs() {
    let fixture = Fixture::new();
    assert_eq!(fixture.install().status.code(), Some(0));
    let allowed = TempDir::new().unwrap();
    let mut child = fixture
        .ashift()
        .arg("mcp")
        .arg("--allow-dir")
        .arg(allowed.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn ashift mcp");
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut send = |value: Value| {
        stdin.write_all(value.to_string().as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
    };
    send(
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2024-11-05", "capabilities": {},
        "clientInfo": {"name": "packs-test", "version": "1.0"}}}),
    );
    send(json!({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}}));
    send(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "list_engines", "arguments": {}}}));
    let response = loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0, "unexpected EOF");
        let message: Value = serde_json::from_str(line.trim()).expect("JSON-RPC line");
        if message["id"] == 2 {
            break message;
        }
    };
    drop(send);
    let _ = child.kill();
    let _ = child.wait();
    let rows = response["result"]["structuredContent"]
        .as_array()
        .expect("list_engines returns rows")
        .clone();
    let row = rows
        .iter()
        .find(|row| row["id"] == "probe")
        .expect("MCP lists the installed pack");
    assert_eq!(row["pack"]["version"], "1.0.0");
}

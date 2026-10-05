use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::error::Error;
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_SUITE_BYTES: u64 = 25 * 1024 * 1024;
const MIN_DOCUMENTS: usize = 50;
const MIN_MARKDOWN_TO_DOCX: usize = 20;

const ALLOWED_LICENSES: &[&str] = &[
    "Apache-2.0",
    "MIT",
    "CC-BY-4.0",
    "CC0-1.0",
    "CC-PDM-1.0",
    "LicenseRef-VN-IPL-Art15",
    "CDLA-Permissive-1.0",
    "ODC-By-1.0",
];

const ALLOWED_TAGS: &[&str] = &[
    "headings",
    "lists-nested",
    "tables",
    "tables-merged",
    "footnotes",
    "math",
    "code",
    "images",
    "links",
    "blockquote",
    "task-list",
    "html-inline",
    "emoji",
    "rtl",
    "cjk",
    "mixed-script",
    "nfd-text",
    "long",
    "multi-column",
    "digital-pdf",
    "scan",
    "low-dpi",
    "skew",
    "handwriting",
    "forms",
    "seal",
    "signed",
    "numbered-articles",
    "verse",
];

#[derive(Clone, Debug, Deserialize)]
pub struct FixtureManifest {
    pub fixture: Vec<Fixture>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Fixture {
    pub id: String,
    pub path: String,
    pub format: String,
    pub media_type: String,
    pub languages: Vec<String>,
    pub title: String,
    pub origin: String,
    pub source_url: Option<String>,
    pub retrieved: Option<String>,
    pub generated_by: Option<String>,
    pub license: String,
    pub license_basis: Option<String>,
    pub attribution: String,
    pub sha256: String,
    pub pages: Option<usize>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub phase: String,
    #[serde(default)]
    pub routes: Vec<String>,
    #[serde(default)]
    pub companions: Vec<Companion>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Companion {
    pub path: String,
    pub sha256: String,
}

pub fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn load_manifest() -> Result<FixtureManifest, Box<dyn Error>> {
    let manifest_path = repository_root().join("fixtures/manifest.toml");
    let contents = fs::read_to_string(manifest_path)?;
    Ok(toml::from_str(&contents)?)
}

pub fn fixtures_for(route: &str, phase: &str) -> Result<Vec<Fixture>, Box<dyn Error>> {
    let manifest = load_manifest()?;
    Ok(manifest
        .fixture
        .into_iter()
        .filter(|fixture| fixture.phase == phase && fixture.routes.iter().any(|item| item == route))
        .collect())
}

pub fn validate_manifest(manifest: &FixtureManifest) -> Vec<String> {
    validate_manifest_at(&repository_root(), manifest)
}

fn validate_manifest_at(root: &Path, manifest: &FixtureManifest) -> Vec<String> {
    let mut errors = Vec::new();
    let mut ids = HashSet::new();
    let mut listed_paths = HashSet::new();
    let mut total_bytes = 0_u64;
    let mut markdown_to_docx_count = 0;

    for fixture in &manifest.fixture {
        if !is_kebab_case(&fixture.id) {
            errors.push(format!("fixture id is not kebab-case: {}", fixture.id));
        }
        if !ids.insert(&fixture.id) {
            errors.push(format!("duplicate fixture id: {}", fixture.id));
        }
        if !ALLOWED_LICENSES.contains(&fixture.license.as_str()) {
            errors.push(format!(
                "fixture {} has disallowed license {}",
                fixture.id, fixture.license
            ));
        }
        for tag in &fixture.tags {
            if !ALLOWED_TAGS.contains(&tag.as_str()) {
                errors.push(format!("fixture {} has unknown tag {tag}", fixture.id));
            }
        }
        match fixture.origin.as_str() {
            "external" => {
                for (name, value) in [
                    ("source_url", fixture.source_url.as_deref()),
                    ("retrieved", fixture.retrieved.as_deref()),
                    ("license_basis", fixture.license_basis.as_deref()),
                ] {
                    if value.is_none_or(|item| item.trim().is_empty()) {
                        errors.push(format!("external fixture {} is missing {name}", fixture.id));
                    }
                }
            }
            "generated" => {
                if fixture
                    .generated_by
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty())
                {
                    errors.push(format!(
                        "generated fixture {} is missing generated_by",
                        fixture.id
                    ));
                }
            }
            other => errors.push(format!("fixture {} has unknown origin {other}", fixture.id)),
        }

        if fixture.routes.iter().any(|route| route == "md->docx") {
            markdown_to_docx_count += 1;
        }

        let files = std::iter::once((&fixture.path, &fixture.sha256)).chain(
            fixture
                .companions
                .iter()
                .map(|item| (&item.path, &item.sha256)),
        );
        for (relative_path, expected_hash) in files {
            if !is_fixture_path(relative_path) {
                errors.push(format!(
                    "fixture {} has unsafe path {relative_path}",
                    fixture.id
                ));
                continue;
            }
            if !listed_paths.insert(relative_path.clone()) {
                errors.push(format!(
                    "fixture path is listed more than once: {relative_path}"
                ));
                continue;
            }

            let (file_path, size) = match safe_fixture_file(root, relative_path) {
                Ok(file) => file,
                Err(error) => {
                    errors.push(format!(
                        "cannot safely access fixture {relative_path}: {error}"
                    ));
                    continue;
                }
            };
            total_bytes = total_bytes.saturating_add(size);
            if size > MAX_FILE_BYTES {
                errors.push(format!(
                    "fixture file exceeds 2 MiB: {relative_path} ({size} bytes)"
                ));
                continue;
            }
            match fs::read(&file_path) {
                Ok(contents) => {
                    let actual_hash = hex::encode(Sha256::digest(&contents));
                    if actual_hash != *expected_hash {
                        errors.push(format!("SHA-256 mismatch for {relative_path}: expected {expected_hash}, got {actual_hash}"));
                    }
                }
                Err(error) => errors.push(format!("cannot read fixture {relative_path}: {error}")),
            }
        }
    }

    if manifest.fixture.len() < MIN_DOCUMENTS {
        errors.push(format!(
            "fixture manifest has {} documents; at least {MIN_DOCUMENTS} are required",
            manifest.fixture.len()
        ));
    }
    if markdown_to_docx_count < MIN_MARKDOWN_TO_DOCX {
        errors.push(format!("fixture manifest has {markdown_to_docx_count} md->docx documents; at least {MIN_MARKDOWN_TO_DOCX} are required"));
    }
    if total_bytes > MAX_SUITE_BYTES {
        errors.push(format!("fixture suite exceeds 25 MiB: {total_bytes} bytes"));
    }

    validate_markdown_cases(root, manifest, &mut errors);
    validate_fixture_tree(root, &listed_paths, &mut errors);
    errors
}

fn is_kebab_case(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn is_fixture_path(value: &str) -> bool {
    if !value.starts_with("fixtures/") || value.contains('\\') {
        return false;
    }
    let path = Path::new(value);
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Normal(name)) if name == "fixtures") {
        return false;
    }
    if components
        .clone()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return false;
    }
    !value.starts_with("fixtures/gen/")
        && !value.starts_with("fixtures/golden/")
        && value != "fixtures/manifest.toml"
}

fn safe_fixture_file(root: &Path, relative_path: &str) -> Result<(PathBuf, u64), String> {
    if !is_fixture_path(relative_path) {
        return Err(
            "path is outside the fixture corpus or reserved for generated files".to_owned(),
        );
    }
    let path = root.join(relative_path);
    let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("path is not a regular file".to_owned());
    }
    let fixture_root =
        fs::canonicalize(root.join("fixtures")).map_err(|error| error.to_string())?;
    let canonical_path = fs::canonicalize(path).map_err(|error| error.to_string())?;
    if !canonical_path.starts_with(fixture_root) {
        return Err("resolved path escapes the fixture corpus".to_owned());
    }
    Ok((canonical_path, metadata.len()))
}

fn validate_markdown_cases(root: &Path, manifest: &FixtureManifest, errors: &mut Vec<String>) {
    let cases = [
        ("vi-nfd", "nfd-text"),
        ("en-crlf", ""),
        ("vi-front-matter", ""),
        ("en-emoji-shortcodes", "emoji"),
        ("vi-local-image", "images"),
    ];
    let mut contents = Vec::new();
    for (id, required_tag) in cases {
        let Some(fixture) = manifest.fixture.iter().find(|fixture| fixture.id == id) else {
            errors.push(format!("required Markdown fixture is missing: {id}"));
            contents.push(None);
            continue;
        };
        if fixture.format != "md" {
            errors.push(format!(
                "required Markdown fixture {id} has format {}",
                fixture.format
            ));
        }
        if !required_tag.is_empty() && !fixture.tags.iter().any(|tag| tag == required_tag) {
            errors.push(format!(
                "required Markdown fixture {id} is missing tag {required_tag}"
            ));
        }
        contents.push(match safe_fixture_file(root, &fixture.path) {
            Ok((path, size)) if size <= MAX_FILE_BYTES => fs::read(path).ok(),
            _ => None,
        });
    }

    if let Some(bytes) = contents[0].as_deref() {
        let has_combining_mark = std::str::from_utf8(bytes).is_ok_and(|text| {
            text.chars()
                .any(|character| ('\u{0300}'..='\u{036f}').contains(&character))
        });
        if !has_combining_mark {
            errors.push("vi-nfd does not contain decomposed combining marks".to_owned());
        }
    }
    if let Some(bytes) = contents[1].as_deref() {
        let has_crlf = bytes.windows(2).any(|pair| pair == b"\r\n");
        let has_bare_lf = bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| *byte == b'\n' && index > 0 && bytes[index - 1] != b'\r');
        if !has_crlf || has_bare_lf {
            errors.push("en-crlf must use CRLF for every line ending".to_owned());
        }
    }
    if let Some(bytes) = contents[2].as_deref()
        && !bytes.starts_with(b"---\n")
        && !bytes.starts_with(b"---\r\n")
    {
        errors.push("vi-front-matter must start with a YAML front-matter delimiter".to_owned());
    }
    if let Some(bytes) = contents[3].as_deref() {
        let text = String::from_utf8_lossy(bytes);
        if !text.contains(":smile:") && !text.contains(":tada:") {
            errors.push("en-emoji-shortcodes is missing a GitHub emoji shortcode".to_owned());
        }
    }
    if let Some(bytes) = contents[4].as_deref() {
        let text = String::from_utf8_lossy(bytes);
        if !text.contains("](assets/") {
            errors.push("vi-local-image is missing a local image reference".to_owned());
        }
    }
}

fn validate_fixture_tree(root: &Path, listed_paths: &HashSet<String>, errors: &mut Vec<String>) {
    let mut actual_paths = HashSet::new();
    if let Err(error) = collect_fixture_paths(&root.join("fixtures"), root, &mut actual_paths) {
        errors.push(format!("cannot inspect fixture tree: {error}"));
        return;
    }
    for path in actual_paths.difference(listed_paths) {
        errors.push(format!(
            "fixture file has no manifest entry or companion: {path}"
        ));
    }
}

fn collect_fixture_paths(
    directory: &Path,
    root: &Path,
    paths: &mut HashSet<String>,
) -> std::io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let relative_path = path
            .strip_prefix(root)
            .expect("fixture paths are below the repository root")
            .to_string_lossy()
            .replace('\\', "/");
        if relative_path == "fixtures/manifest.toml"
            || relative_path == "fixtures/gen"
            || relative_path.starts_with("fixtures/gen/")
            || relative_path == "fixtures/golden"
            || relative_path.starts_with("fixtures/golden/")
        {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_fixture_paths(&path, root, paths)?;
        } else if file_type.is_file() || file_type.is_symlink() {
            paths.insert(relative_path);
        }
    }
    Ok(())
}

//! The `ariad-pack/1` manifest: model, validation and `{root}` expansion.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    path::{Path, PathBuf},
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::PackError;
use crate::engines::{self, Protocol};

/// Manifest schema identifier.
pub const MANIFEST_SCHEMA: &str = "ariad-pack/1";
/// File name of a pack manifest.
pub const MANIFEST_FILE: &str = "pack.toml";
/// File name of the detached minisign signature of a pack manifest.
pub const SIGNATURE_FILE: &str = "pack.toml.minisig";
/// Upper bound for a manifest file.
pub const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
/// Placeholder replaced by the absolute install root in entrypoint arguments, `env` and `options`.
pub const ROOT_PLACEHOLDER: &str = "{root}";

/// Engine settings a manifest may place in the engine environment.
///
/// The runner clears the environment of every engine; only these names pass from a manifest.
pub const ALLOWED_ENV: &[&str] = &[
    "DO_NOT_TRACK",
    "HF_DATASETS_OFFLINE",
    "HF_HUB_DISABLE_TELEMETRY",
    "HF_HUB_OFFLINE",
    "MKL_NUM_THREADS",
    "OMP_NUM_THREADS",
    "OPENBLAS_NUM_THREADS",
    "PYTHONDONTWRITEBYTECODE",
    "PYTHONHASHSEED",
    "PYTHONHOME",
    "PYTHONIOENCODING",
    "PYTHONNOUSERSITE",
    "PYTHONSAFEPATH",
    "PYTHONUTF8",
    "TESSDATA_PREFIX",
    "TOKENIZERS_PARALLELISM",
    "TRANSFORMERS_OFFLINE",
];

/// File names at the top of an install root that belong to the host, never to a component.
pub(crate) const RESERVED_ROOT_NAMES: &[&str] = &["receipt.toml"];

const MAX_NAME_LEN: usize = 64;
const MAX_RELATIVE_PATH_BYTES: usize = 4096;

/// A signed engine pack manifest (`ariad-pack/1`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Engine pack manifest")]
pub struct Manifest {
    /// Manifest schema identifier, always `"ariad-pack/1"`.
    pub schema: String,
    /// Pack name: lower-case ASCII letters, digits and `-`, starting with a letter.
    pub name: String,
    /// Pack version (SemVer).
    pub version: String,
    /// One-line description.
    #[serde(default)]
    pub description: String,
    /// SPDX licence summary of everything the pack contains.
    pub license: String,
    /// SemVer requirement on the `ashift` version that may install and run this pack.
    pub requires_ashift: String,
    /// Wire protocol the pack's engine speaks (`"ariad-engine/1"` or `"ariad-engine/2"`).
    /// Absent means `"ariad-engine/1"`. Install and resolve refuse a pack whose value differs
    /// from the protocol the host registers for the engine of the same name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_protocol: Option<String>,
    /// Licence and notice files inside the install root, as relative paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<String>,
    /// Per-platform builds of the pack.
    pub targets: Vec<Target>,
}

/// One platform build of a pack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Target {
    /// Rust target triple of the pack's binaries, e.g. `x86_64-unknown-linux-gnu`.
    pub triple: String,
    /// Engine argv. The first element is a path relative to the install root; later elements
    /// may contain `{root}`.
    pub entrypoint: Vec<String>,
    /// Executable paths inside archive components, relative to the install root.
    /// When absent or empty, resolve allows only `entrypoint[0]` and `unpack = "file"`
    /// components with `executable = true`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub executables: Vec<String>,
    /// Allow-listed engine environment variables; values may contain `{root}`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Default request options for the engine; string values may contain `{root}`.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub options: Map<String, Value>,
    /// Files that make up this build.
    #[serde(default)]
    pub components: Vec<Component>,
}

/// One downloadable file of a pack build.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Component {
    /// Component name, unique within its target.
    pub name: String,
    /// `https://` URL, or a path relative to the manifest's location.
    pub url: String,
    /// Lower-case hex SHA-256 of the downloaded file.
    pub sha256: String,
    /// Size of the downloaded file in bytes.
    pub size: u64,
    /// How the file is unpacked.
    pub unpack: Unpack,
    /// Directory inside the install root that receives the content (`.` for the root).
    #[serde(default = "default_dest")]
    pub dest: String,
    /// Sum of the regular file sizes inside an archive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unpacked_size: Option<u64>,
    /// Number of entries (files, directories and links) inside an archive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entries: Option<u64>,
    /// For `unpack = "file"`: mark the installed file executable.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub executable: bool,
}

fn default_dest() -> String {
    ".".to_owned()
}

/// Unpack mode of a component.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum Unpack {
    /// Zstandard-compressed tar archive.
    #[serde(rename = "tar.zst")]
    TarZst,
    /// Gzip-compressed tar archive.
    #[serde(rename = "tar.gz")]
    TarGz,
    /// ZIP archive.
    #[serde(rename = "zip")]
    Zip,
    /// A single file written to `<dest>/<name>`.
    #[serde(rename = "file")]
    File,
}

impl Unpack {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TarZst => "tar.zst",
            Self::TarGz => "tar.gz",
            Self::Zip => "zip",
            Self::File => "file",
        }
    }

    #[must_use]
    pub const fn is_archive(self) -> bool {
        !matches!(self, Self::File)
    }

    /// Detects the unpack mode from a file name.
    #[must_use]
    pub fn from_file_name(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".tar.zst") || lower.ends_with(".tzst") {
            Self::TarZst
        } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
            Self::TarGz
        } else if lower.ends_with(".zip") {
            Self::Zip
        } else {
            Self::File
        }
    }
}

/// The fields checked before the strict parse, so a newer manifest yields a version error
/// instead of an unknown-field error.
#[derive(Debug, Deserialize)]
pub(crate) struct ManifestHeader {
    pub schema: String,
    pub name: String,
    pub version: String,
    pub requires_ashift: String,
}

impl ManifestHeader {
    pub(crate) fn parse(text: &str) -> Result<Self, PackError> {
        toml::from_str(text).map_err(|error| PackError::InvalidManifest(error.to_string()))
    }
}

impl Manifest {
    /// Parses and validates a manifest.
    pub fn parse(text: &str) -> Result<Self, PackError> {
        let header = ManifestHeader::parse(text)?;
        if header.schema != MANIFEST_SCHEMA {
            return Err(PackError::InvalidManifest(format!(
                "unsupported schema '{}'; this ashift reads '{MANIFEST_SCHEMA}'",
                header.schema
            )));
        }
        let manifest: Self =
            toml::from_str(text).map_err(|error| PackError::InvalidManifest(error.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Serializes the manifest as TOML.
    pub fn to_toml(&self) -> Result<String, PackError> {
        toml::to_string(self).map_err(|error| PackError::InvalidManifest(error.to_string()))
    }

    /// Checks every structural rule of `ariad-pack/1`.
    pub fn validate(&self) -> Result<(), PackError> {
        self.validate_inner().map_err(PackError::InvalidManifest)
    }

    fn validate_inner(&self) -> Result<(), String> {
        if self.schema != MANIFEST_SCHEMA {
            return Err(format!("schema must be '{MANIFEST_SCHEMA}'"));
        }
        validate_pack_name(&self.name)?;
        semver::Version::parse(&self.version)
            .map_err(|error| format!("version '{}' is not SemVer: {error}", self.version))?;
        if self.license.trim().is_empty() {
            return Err("license must not be empty".to_owned());
        }
        semver::VersionReq::parse(&self.requires_ashift).map_err(|error| {
            format!(
                "requires_ashift '{}' is not a SemVer requirement: {error}",
                self.requires_ashift
            )
        })?;
        self.check_engine_protocol()?;
        for notice in &self.notices {
            relative_path(notice).map_err(|reason| format!("notice '{notice}': {reason}"))?;
        }
        if self.targets.is_empty() {
            return Err("at least one [[targets]] entry is required".to_owned());
        }
        let mut triples = BTreeSet::new();
        for target in &self.targets {
            if !triples.insert(target.triple.as_str()) {
                return Err(format!("target '{}' is listed twice", target.triple));
            }
            target
                .validate()
                .map_err(|reason| format!("target '{}': {reason}", target.triple))?;
        }
        Ok(())
    }

    /// The protocol the pack declares; an absent `engine_protocol` means `ariad-engine/1`.
    fn declared_protocol(&self) -> Result<Protocol, String> {
        match self.engine_protocol.as_deref() {
            None => Ok(Protocol::V1),
            Some(value) if value == Protocol::V1.as_str() => Ok(Protocol::V1),
            Some(value) if value == Protocol::V2.as_str() => Ok(Protocol::V2),
            Some(value) => Err(format!(
                "engine_protocol '{value}' is not supported; use '{}' or '{}'",
                Protocol::V1.as_str(),
                Protocol::V2.as_str()
            )),
        }
    }

    /// Refuses a pack whose declared protocol differs from the one the host registers for the
    /// engine it provides (`pack name == engine id`). Packs for unregistered names are not
    /// checked.
    fn check_engine_protocol(&self) -> Result<(), String> {
        let declared = self.declared_protocol()?;
        match engines::pack_spec(&self.name) {
            Some(spec) if spec.protocol != declared => Err(format!(
                "engine_protocol is '{}'{} but the '{}' engine speaks '{}'; rebuild the pack \
                 with engine_protocol = \"{}\"",
                declared.as_str(),
                if self.engine_protocol.is_none() {
                    " (the default when the field is absent)"
                } else {
                    ""
                },
                spec.id,
                spec.protocol.as_str(),
                spec.protocol.as_str()
            )),
            _ => Ok(()),
        }
    }

    /// Returns the build for the first matching triple in `preferred`.
    #[must_use]
    pub fn select_target(&self, preferred: &[String]) -> Option<&Target> {
        preferred
            .iter()
            .find_map(|triple| self.targets.iter().find(|target| &target.triple == triple))
    }

    /// Comma-separated list of the triples this manifest provides.
    #[must_use]
    pub fn triples(&self) -> String {
        self.targets
            .iter()
            .map(|target| target.triple.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Checks `requires_ashift` against the running `ashift`.
    pub(crate) fn check_ashift(&self) -> Result<(), PackError> {
        check_requires_ashift(&self.name, &self.version, &self.requires_ashift)
    }
}

pub(crate) fn check_requires_ashift(
    name: &str,
    version: &str,
    requires: &str,
) -> Result<(), PackError> {
    let requirement = semver::VersionReq::parse(requires).map_err(|error| {
        PackError::InvalidManifest(format!(
            "requires_ashift '{requires}' is not a SemVer requirement: {error}"
        ))
    })?;
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))
        .expect("the crate version is valid SemVer");
    if requirement.matches(&current) {
        Ok(())
    } else {
        Err(PackError::IncompatibleAshift {
            name: name.to_owned(),
            version: version.to_owned(),
            requires: requires.to_owned(),
            current: current.to_string(),
        })
    }
}

impl Target {
    fn validate(&self) -> Result<(), String> {
        if self.triple.is_empty()
            || !self
                .triple
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
        {
            return Err("triple must be a Rust target triple".to_owned());
        }
        let Some(program) = self.entrypoint.first() else {
            return Err("entrypoint must name a program".to_owned());
        };
        relative_path(program).map_err(|reason| format!("entrypoint '{program}': {reason}"))?;
        for exec in &self.executables {
            relative_path(exec).map_err(|reason| format!("executable '{exec}': {reason}"))?;
        }
        for name in self.env.keys() {
            if !ALLOWED_ENV.contains(&name.as_str()) {
                return Err(format!(
                    "env variable '{name}' is not allowed; allowed: {}",
                    ALLOWED_ENV.join(", ")
                ));
            }
        }
        if self.components.is_empty() {
            return Err("at least one component is required".to_owned());
        }
        let mut names = BTreeSet::new();
        for component in &self.components {
            if !names.insert(component.name.as_str()) {
                return Err(format!("component '{}' is listed twice", component.name));
            }
            component
                .validate()
                .map_err(|reason| format!("component '{}': {reason}", component.name))?;
        }
        Ok(())
    }
}

impl Component {
    fn validate(&self) -> Result<(), String> {
        validate_component_name(&self.name)?;
        if self.url.is_empty() {
            return Err("url must not be empty".to_owned());
        }
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("sha256 must be 64 lower-case hex digits".to_owned());
        }
        if self.dest != "." {
            relative_path(&self.dest).map_err(|reason| format!("dest: {reason}"))?;
        }
        if self.unpack.is_archive() {
            if self.unpacked_size.is_none() || self.entries.is_none() {
                return Err(format!(
                    "unpack = \"{}\" requires unpacked_size and entries",
                    self.unpack.as_str()
                ));
            }
            if self.executable {
                return Err("executable applies to unpack = \"file\" only".to_owned());
            }
        } else if self.unpacked_size.is_some() || self.entries.is_some() {
            return Err("unpacked_size and entries apply to archives only".to_owned());
        }
        Ok(())
    }

    /// Relative path of the installed file for `unpack = "file"`.
    #[must_use]
    pub fn file_path(&self) -> String {
        if self.dest == "." {
            self.name.clone()
        } else {
            format!("{}/{}", self.dest, self.name)
        }
    }
}

/// Validates a pack name: `[a-z][a-z0-9-]*`, at most 64 bytes, no trailing `-`.
pub fn validate_pack_name(name: &str) -> Result<(), String> {
    let valid = !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name.as_bytes()[0].is_ascii_lowercase()
        && !name.ends_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if valid {
        Ok(())
    } else {
        Err(format!(
            "invalid pack name '{name}'; use lower-case letters, digits and '-'"
        ))
    }
}

pub(crate) fn validate_component_name(name: &str) -> Result<(), String> {
    let valid = !name.is_empty()
        && name.len() <= 128
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        && !name.contains("..");
    if valid {
        Ok(())
    } else {
        Err(format!(
            "invalid component name '{name}'; use ASCII letters, digits, '.', '_' and '-'"
        ))
    }
}

/// Validates a portable relative path with `/` separators and returns its components.
///
/// Rejects empty paths, absolute paths, `..`, `.`, empty components, backslashes, NUL and
/// other control characters, and drive or stream syntax (`:`).
pub(crate) fn relative_path(path: &str) -> Result<Vec<&str>, &'static str> {
    if path.is_empty() {
        return Err("path is empty");
    }
    if path.len() > MAX_RELATIVE_PATH_BYTES {
        return Err("path is too long");
    }
    if path.starts_with('/') {
        return Err("path is absolute");
    }
    if path
        .chars()
        .any(|c| c == '\\' || c == ':' || c.is_control())
    {
        return Err("path contains a backslash, ':' or a control character");
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" => return Err("path contains an empty component"),
            "." => return Err("path contains '.'"),
            ".." => return Err("path contains '..'"),
            _ => parts.push(part),
        }
    }
    Ok(parts)
}

/// Joins validated `/`-separated components onto `base`.
pub(crate) fn join_relative(base: &Path, parts: &[&str]) -> PathBuf {
    let mut path = base.to_path_buf();
    for part in parts {
        path.push(part);
    }
    path
}

/// Replaces `{root}` with the install root.
pub(crate) fn expand_os(value: &str, root: &Path) -> OsString {
    let mut out = OsString::new();
    let mut rest = value;
    while let Some(index) = rest.find(ROOT_PLACEHOLDER) {
        out.push(&rest[..index]);
        out.push(root.as_os_str());
        rest = &rest[index + ROOT_PLACEHOLDER.len()..];
    }
    out.push(rest);
    out
}

/// Replaces `{root}` in every string of a JSON value. Fails when the root is not UTF-8 and a
/// value needs it.
pub(crate) fn expand_json(value: &Value, root: &Path) -> Result<Value, PackError> {
    Ok(match value {
        Value::String(text) if text.contains(ROOT_PLACEHOLDER) => {
            let root = root.to_str().ok_or_else(|| {
                PackError::InvalidManifest(
                    "the install root is not valid UTF-8, so options cannot reference {root}"
                        .to_owned(),
                )
            })?;
            Value::String(text.replace(ROOT_PLACEHOLDER, root))
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| expand_json(item, root))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| Ok((key.clone(), expand_json(item, root)?)))
                .collect::<Result<_, PackError>>()?,
        ),
        other => other.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const SAMPLE: &str = r#"
schema = "ariad-pack/1"
name = "sample"
version = "1.2.3"
description = "Sample pack"
license = "MIT"
requires_ashift = ">=0.1.0"
notices = ["notices/LICENSE.txt"]

[[targets]]
triple = "x86_64-unknown-linux-gnu"
entrypoint = ["bin/engine", "--root", "{root}"]
env = { HF_HUB_OFFLINE = "1", TESSDATA_PREFIX = "{root}/tessdata" }
options = { artifacts_path = "{root}/models", nested = { list = ["{root}/a", 3] } }

[[targets.components]]
name = "runtime"
url = "runtime.tar.zst"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
size = 10
unpack = "tar.zst"
dest = "runtime"
unpacked_size = 100
entries = 4
"#;

    #[test]
    fn parses_and_round_trips_a_manifest() {
        let manifest = Manifest::parse(SAMPLE).expect("parse sample");
        assert_eq!(manifest.name, "sample");
        assert_eq!(manifest.targets[0].components[0].unpack, Unpack::TarZst);
        let text = manifest.to_toml().expect("serialize");
        assert_eq!(Manifest::parse(&text).expect("reparse"), manifest);
    }

    #[test]
    fn rejects_unknown_fields_and_bad_values() {
        let unknown = SAMPLE.replace("description =", "surprise = 1\ndescription =");
        assert!(matches!(
            Manifest::parse(&unknown),
            Err(PackError::InvalidManifest(_))
        ));
        let bad_env = SAMPLE.replace("HF_HUB_OFFLINE", "LD_PRELOAD");
        let error = Manifest::parse(&bad_env).unwrap_err().to_string();
        assert!(error.contains("LD_PRELOAD"), "{error}");
        let traversal = SAMPLE.replace("dest = \"runtime\"", "dest = \"../runtime\"");
        assert!(Manifest::parse(&traversal).is_err());
        let absolute = SAMPLE.replace("bin/engine", "/bin/engine");
        assert!(Manifest::parse(&absolute).is_err());
        let no_sizes = SAMPLE.replace("unpacked_size = 100\n", "");
        assert!(Manifest::parse(&no_sizes).is_err());
        let bad_version = SAMPLE.replace("1.2.3", "1.2");
        assert!(Manifest::parse(&bad_version).is_err());
        let newer = SAMPLE.replace("ariad-pack/1", "ariad-pack/2");
        let error = Manifest::parse(&newer).unwrap_err().to_string();
        assert!(error.contains("ariad-pack/2"), "{error}");
    }

    fn named(name: &str, engine_protocol: Option<&str>) -> String {
        let text = SAMPLE.replace("name = \"sample\"", &format!("name = \"{name}\""));
        match engine_protocol {
            Some(value) => text.replace(
                "requires_ashift = \">=0.1.0\"",
                &format!("requires_ashift = \">=0.1.0\"\nengine_protocol = \"{value}\""),
            ),
            None => text,
        }
    }

    #[test]
    fn engine_protocol_is_optional_and_round_trips() {
        let absent = Manifest::parse(SAMPLE).expect("manifest without the field");
        assert_eq!(absent.engine_protocol, None);
        assert!(!absent.to_toml().unwrap().contains("engine_protocol"));
        for value in ["ariad-engine/1", "ariad-engine/2"] {
            let manifest = Manifest::parse(&named("sample", Some(value))).expect(value);
            assert_eq!(manifest.engine_protocol.as_deref(), Some(value));
            let text = manifest.to_toml().expect("serialize");
            assert!(
                text.contains(&format!("engine_protocol = \"{value}\"")),
                "{text}"
            );
            assert_eq!(Manifest::parse(&text).expect("reparse"), manifest);
        }
    }

    #[test]
    fn unknown_engine_protocol_values_are_refused() {
        for value in [
            "ariad-engine/3",
            "ariad-engine/0",
            "",
            "2",
            "ARIAD-ENGINE/2",
        ] {
            let error = Manifest::parse(&named("sample", Some(value)))
                .expect_err(value)
                .to_string();
            assert!(error.contains("engine_protocol"), "{value}: {error}");
        }
    }

    #[test]
    fn engine_protocol_must_match_the_registered_engine() {
        // pdfium is registered as ariad-engine/2: the field is mandatory and must say so.
        assert!(Manifest::parse(&named("pdfium", Some("ariad-engine/2"))).is_ok());
        for declared in [None, Some("ariad-engine/1")] {
            let error = Manifest::parse(&named("pdfium", declared))
                .expect_err("a v2 engine needs engine_protocol = ariad-engine/2")
                .to_string();
            assert!(
                error.contains("'pdfium' engine speaks 'ariad-engine/2'"),
                "{error}"
            );
        }
        // docling is registered as ariad-engine/1: existing manifests without the field stay valid.
        assert!(Manifest::parse(&named("docling", None)).is_ok());
        assert!(Manifest::parse(&named("docling", Some("ariad-engine/1"))).is_ok());
        let error = Manifest::parse(&named("docling", Some("ariad-engine/2")))
            .expect_err("a v1 engine must not claim v2")
            .to_string();
        assert!(
            error.contains("'docling' engine speaks 'ariad-engine/1'"),
            "{error}"
        );
    }

    #[test]
    fn pack_libraries_cannot_widen_the_engine_environment() {
        for name in ["LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH", "PDFIUM_PATH"] {
            let text = named("pdfium", Some("ariad-engine/2")).replace("HF_HUB_OFFLINE", name);
            let error = Manifest::parse(&text).expect_err(name).to_string();
            assert!(error.contains(name), "{error}");
        }
    }

    #[test]
    fn relative_paths_reject_escapes() {
        assert_eq!(relative_path("a/b").unwrap(), vec!["a", "b"]);
        for bad in [
            "", "/a", "a/../b", "..", "a//b", "a/./b", "a\\b", "c:x", "a\0b",
        ] {
            assert!(relative_path(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn expands_root_placeholders() {
        let root = Path::new("/opt/pack");
        assert_eq!(
            expand_os("{root}/x:{root}", root),
            OsString::from("/opt/pack/x:/opt/pack")
        );
        let manifest = Manifest::parse(SAMPLE).unwrap();
        let options = Value::Object(manifest.targets[0].options.clone());
        let expanded = expand_json(&options, root).unwrap();
        assert_eq!(expanded["artifacts_path"], "/opt/pack/models");
        assert_eq!(expanded["nested"]["list"][0], "/opt/pack/a");
        assert_eq!(expanded["nested"]["list"][1], 3);
    }

    #[test]
    fn pack_names_are_strict() {
        for good in ["docling", "ocr-extra", "a1"] {
            assert!(validate_pack_name(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "Docling",
            "1pack",
            "pack-",
            "pa_ck",
            "../x",
            &"a".repeat(65),
        ] {
            assert!(validate_pack_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn unpack_mode_follows_the_extension() {
        assert_eq!(Unpack::from_file_name("a.tar.zst"), Unpack::TarZst);
        assert_eq!(Unpack::from_file_name("a.TGZ"), Unpack::TarGz);
        assert_eq!(Unpack::from_file_name("a.zip"), Unpack::Zip);
        assert_eq!(Unpack::from_file_name("engine"), Unpack::File);
    }
}

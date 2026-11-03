//! Validate pack-backed adapter executables against the installed manifest.
use super::common::EngineFailure;
use std::{
    io::Read,
    path::{Component, Path, PathBuf},
};

const MAX_PACK_METADATA: u64 = 8 * 1024 * 1024;

fn metadata(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_PACK_METADATA + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_PACK_METADATA {
        return None;
    }
    String::from_utf8(bytes).ok()
}

pub(super) fn program(
    name: &str,
    variable: &str,
    message: &'static str,
) -> Result<(PathBuf, PathBuf), EngineFailure> {
    let missing = || EngineFailure::tool_missing(message);
    let p = PathBuf::from(std::env::var_os(variable).ok_or_else(missing)?);
    let root = PathBuf::from(std::env::var_os("ASHIFT_ADAPTER_PACK_ROOT").ok_or_else(missing)?);
    if !p.is_absolute() || !root.is_absolute() {
        return Err(missing());
    }
    let root = root.canonicalize().map_err(|_| missing())?;
    let canonical = p.canonicalize().map_err(|_| missing())?;
    if !canonical.is_file() || !canonical.starts_with(&root) {
        return Err(missing());
    }
    let manifest_text = if root.join("receipt.toml").is_file() {
        let receipt = metadata(&root.join("receipt.toml")).ok_or_else(missing)?;
        let receipt: toml::Value = toml::from_str(&receipt).map_err(|_| missing())?;
        receipt
            .get("manifest")
            .and_then(toml::Value::as_str)
            .ok_or_else(missing)?
            .to_owned()
    } else {
        metadata(&root.join("pack.toml")).ok_or_else(missing)?
    };
    let manifest = crate::packs::Manifest::parse(&manifest_text).map_err(|_| missing())?;
    if manifest.name != name {
        return Err(missing());
    }
    let target = manifest
        .select_target(&crate::packs::host_targets())
        .ok_or_else(missing)?;
    let relative = Path::new(target.entrypoint.first().ok_or_else(missing)?);
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
        || root.join(relative).canonicalize().map_err(|_| missing())? != canonical
    {
        return Err(missing());
    }
    Ok((canonical, root))
}

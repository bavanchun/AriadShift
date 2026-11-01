//! Generates pack manifests from built component files (`ashift __pack manifest`).

use std::{
    fs,
    path::{Path, PathBuf},
};

use tokio_util::sync::CancellationToken;

use super::{
    PackError,
    extract::scan,
    fetch::sha256_file,
    manifest::{
        Component, MANIFEST_SCHEMA, Manifest, Target, Unpack, validate_component_name,
        validate_pack_name,
    },
};

/// Inputs of `ashift __pack manifest`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ManifestRequest {
    pub name: String,
    pub version: String,
    /// Target triple whose components are (re)written.
    pub target: String,
    /// Existing manifest or template providing the pack fields and other targets.
    pub base: Option<PathBuf>,
    /// Entrypoint argv for the target; keeps the base's entrypoint when empty.
    pub entrypoint: Vec<String>,
    /// Prefix for component URLs; the bare file name (relative to the manifest) otherwise.
    pub url_prefix: Option<String>,
    /// `(component, dest)` pairs; components default to the install root.
    pub dests: Vec<(String, String)>,
    /// `(name, file)` pairs; the name defaults to the file name without archive extensions.
    pub components: Vec<(Option<String>, PathBuf)>,
}

/// Builds an unsigned manifest whose `target` lists the given component files with their size,
/// SHA-256, unpack mode and, for archives, unpacked size and entry count.
pub fn generate_manifest(request: &ManifestRequest) -> Result<Manifest, PackError> {
    validate_pack_name(&request.name).map_err(PackError::Usage)?;
    semver::Version::parse(&request.version).map_err(|error| {
        PackError::Usage(format!("invalid version '{}': {error}", request.version))
    })?;
    if request.components.is_empty() {
        return Err(PackError::Usage(
            "at least one component file is required".to_owned(),
        ));
    }
    let mut manifest = match &request.base {
        Some(path) => {
            let text = fs::read_to_string(path)
                .map_err(|error| PackError::io("could not read", path, error))?;
            let base: Manifest = toml::from_str(&text).map_err(|error| {
                PackError::InvalidManifest(format!("{}: {error}", path.display()))
            })?;
            if base.schema != MANIFEST_SCHEMA {
                return Err(PackError::InvalidManifest(format!(
                    "{}: schema must be '{MANIFEST_SCHEMA}'",
                    path.display()
                )));
            }
            base
        }
        None => Manifest {
            schema: MANIFEST_SCHEMA.to_owned(),
            name: request.name.clone(),
            version: request.version.clone(),
            description: String::new(),
            license: "NOASSERTION".to_owned(),
            requires_ashift: format!(">={}", env!("CARGO_PKG_VERSION")),
            engine_protocol: None,
            notices: Vec::new(),
            targets: Vec::new(),
        },
    };
    manifest.name.clone_from(&request.name);
    manifest.version.clone_from(&request.version);

    let index = match manifest
        .targets
        .iter()
        .position(|target| target.triple == request.target)
    {
        Some(index) => index,
        None => {
            manifest.targets.push(Target {
                triple: request.target.clone(),
                entrypoint: Vec::new(),
                env: Default::default(),
                options: Default::default(),
                components: Vec::new(),
            });
            manifest.targets.len() - 1
        }
    };
    let target = &mut manifest.targets[index];
    if !request.entrypoint.is_empty() {
        target.entrypoint.clone_from(&request.entrypoint);
    }
    if target.entrypoint.is_empty() {
        return Err(PackError::Usage(format!(
            "target '{}' has no entrypoint; pass --entrypoint or a --base manifest that defines one",
            request.target
        )));
    }
    let cancel = CancellationToken::new();
    target.components = request
        .components
        .iter()
        .map(|(name, path)| describe_component(request, name.as_deref(), path, &cancel))
        .collect::<Result<_, _>>()?;
    manifest.validate()?;
    Ok(manifest)
}

fn describe_component(
    request: &ManifestRequest,
    name: Option<&str>,
    path: &Path,
    cancel: &CancellationToken,
) -> Result<Component, PackError> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| PackError::Usage(format!("{} has no UTF-8 file name", path.display())))?;
    let unpack = Unpack::from_file_name(file_name);
    let name = name.map_or_else(|| component_name(file_name, unpack), str::to_owned);
    validate_component_name(&name).map_err(PackError::Usage)?;
    let (sha256, size) = sha256_file(path, cancel)?;
    let (unpacked_size, entries) = if unpack.is_archive() {
        let stats = scan(path, unpack, &name)?;
        (Some(stats.bytes), Some(stats.entries))
    } else {
        (None, None)
    };
    let url = match &request.url_prefix {
        Some(prefix) if prefix.ends_with('/') => format!("{prefix}{file_name}"),
        Some(prefix) => format!("{prefix}/{file_name}"),
        None => file_name.to_owned(),
    };
    let dest = request
        .dests
        .iter()
        .find(|(component, _)| component == &name)
        .map_or_else(|| ".".to_owned(), |(_, dest)| dest.clone());
    Ok(Component {
        executable: !unpack.is_archive() && is_executable(path),
        name,
        url,
        sha256,
        size,
        unpack,
        dest,
        unpacked_size,
        entries,
    })
}

fn component_name(file_name: &str, unpack: Unpack) -> String {
    let lower = file_name.to_ascii_lowercase();
    let suffixes: &[&str] = match unpack {
        Unpack::TarZst => &[".tar.zst", ".tzst"],
        Unpack::TarGz => &[".tar.gz", ".tgz"],
        Unpack::Zip => &[".zip"],
        Unpack::File => &[],
    };
    suffixes
        .iter()
        .find(|suffix| lower.ends_with(*suffix))
        .map_or_else(
            || file_name.to_owned(),
            |suffix| file_name[..file_name.len() - suffix.len()].to_owned(),
        )
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_with_base(
        directory: &Path,
        name: &str,
        engine_protocol: Option<&str>,
    ) -> ManifestRequest {
        let engine = directory.join("engine");
        fs::write(&engine, b"binary").unwrap();
        let protocol = engine_protocol
            .map(|value| format!("engine_protocol = \"{value}\"\n"))
            .unwrap_or_default();
        let base = directory.join("base.toml");
        fs::write(
            &base,
            format!(
                "schema = \"ariad-pack/1\"\nname = \"{name}\"\nversion = \"0.1.0\"\n\
                 license = \"MIT\"\nrequires_ashift = \">=0.1.0\"\n{protocol}targets = []\n"
            ),
        )
        .unwrap();
        ManifestRequest {
            name: name.to_owned(),
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            base: Some(base),
            entrypoint: vec!["engine".to_owned()],
            components: vec![(None, engine)],
            ..ManifestRequest::default()
        }
    }

    #[test]
    fn engine_protocol_in_the_base_manifest_is_emitted() {
        let directory = tempfile::tempdir().unwrap();
        let request = request_with_base(directory.path(), "pdfium", Some("ariad-engine/2"));
        let manifest = generate_manifest(&request).expect("matching protocol");
        assert_eq!(manifest.engine_protocol.as_deref(), Some("ariad-engine/2"));
        let text = manifest.to_toml().unwrap();
        assert!(
            text.contains("engine_protocol = \"ariad-engine/2\""),
            "{text}"
        );
    }

    #[test]
    fn engine_protocol_mismatch_is_refused_when_generating() {
        let directory = tempfile::tempdir().unwrap();
        let request = request_with_base(directory.path(), "pdfium", None);
        let error = generate_manifest(&request).expect_err("v2 engine without the field");
        assert!(error.to_string().contains("engine_protocol"), "{error}");
        let docling = request_with_base(directory.path(), "docling", None);
        let manifest = generate_manifest(&docling).expect("docling stays on ariad-engine/1");
        assert_eq!(manifest.engine_protocol, None);
    }

    #[test]
    fn component_names_drop_archive_extensions() {
        assert_eq!(component_name("runtime.tar.zst", Unpack::TarZst), "runtime");
        assert_eq!(component_name("models.TGZ", Unpack::TarGz), "models");
        assert_eq!(component_name("tess.zip", Unpack::Zip), "tess");
        assert_eq!(component_name("engine.exe", Unpack::File), "engine.exe");
    }
}

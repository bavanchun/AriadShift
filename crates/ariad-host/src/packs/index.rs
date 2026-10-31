//! The embedded index of packs known by name.

use std::sync::OnceLock;

use serde::Deserialize;

use super::PackError;

const INDEX_SCHEMA: &str = "ariad-pack-index/1";
const INDEX_TOML: &str = include_str!("../../data/packs.toml");

/// A pack that can be installed by name.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct IndexEntry {
    pub name: String,
    /// Recommended version.
    pub version: String,
    pub description: String,
    /// Manifest URL template with `{name}` and `{version}`.
    pub manifest: String,
}

impl IndexEntry {
    /// Manifest URL for `version`.
    #[must_use]
    pub fn manifest_url(&self, version: &str) -> String {
        self.manifest
            .replace("{name}", &self.name)
            .replace("{version}", version)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    schema: String,
    packs: Vec<IndexEntry>,
}

/// Every pack in the embedded index.
#[must_use]
pub fn entries() -> &'static [IndexEntry] {
    static INDEX: OnceLock<Vec<IndexEntry>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let index: Index = toml::from_str(INDEX_TOML).expect("data/packs.toml is valid");
        assert_eq!(index.schema, INDEX_SCHEMA, "data/packs.toml schema");
        index.packs
    })
}

/// Looks up a pack by name.
pub fn lookup(name: &str) -> Result<&'static IndexEntry, PackError> {
    entries()
        .iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| PackError::UnknownPack {
            name: name.to_owned(),
            known: entries()
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packs::manifest::validate_pack_name;

    #[test]
    fn embedded_index_is_valid() {
        assert!(!entries().is_empty());
        for entry in entries() {
            validate_pack_name(&entry.name).unwrap();
            semver::Version::parse(&entry.version).unwrap();
            let url = entry.manifest_url(&entry.version);
            assert!(url.starts_with("https://"), "{url}");
            assert!(!url.contains('{'), "{url}");
        }
    }

    #[test]
    fn docling_manifest_url_follows_the_release_tag_scheme() {
        let docling = lookup("docling").unwrap();
        assert_eq!(
            docling.manifest_url("1.2.3"),
            "https://github.com/bavanchun/AriadShift/releases/download/pack-docling-v1.2.3/pack.toml"
        );
        assert!(matches!(lookup("nope"), Err(PackError::UnknownPack { .. })));
    }
}

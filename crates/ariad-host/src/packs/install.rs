//! Install, remove, verify and resolve packs under `<data>/packs`.
//!
//! Layout:
//!
//! ```text
//! <data>/packs/.locks/<name>.lock   exclusive while installing or removing <name>
//! <data>/packs/.staging/<name>.<id> extraction targets, renamed into place on success
//! <data>/packs/.cache/<sha256>      downloaded, hash-verified components (resume)
//! <data>/packs/<name>/current       active version
//! <data>/packs/<name>/<version>/    install root with receipt.toml
//! ```

use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions, TryLockError},
    io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::{
    InstalledPack, PackContext, PackError, PackSource,
    extract::{self, Bounds, FileRecord, LinkRecord},
    fetch::{self, Fetcher, Location},
    index,
    manifest::{
        MANIFEST_FILE, MAX_MANIFEST_BYTES, Manifest, ManifestHeader, SIGNATURE_FILE,
        check_requires_ashift, relative_path, validate_pack_name,
    },
    signature::MAX_SIGNATURE_BYTES,
};

const RECEIPT_FILE: &str = "receipt.toml";
const RECEIPT_SCHEMA: &str = "ariad-pack-receipt/1";
const CURRENT_FILE: &str = "current";
const STALE_PART_AGE: Duration = Duration::from_secs(24 * 3600);
const STALE_CACHE_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

/// Where `install` reads the manifest from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstallSource {
    /// The embedded index's manifest URL.
    Index,
    /// An explicit manifest URL or path; the signature is the same location plus `.minisig`.
    Manifest(String),
    /// An offline directory holding `pack.toml`, `pack.toml.minisig` and the component files,
    /// each named after the last segment of its `url`.
    Directory(PathBuf),
}

/// An install request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallRequest {
    pub name: String,
    /// Required version; the index's recommended version when `None` and the source is
    /// [`InstallSource::Index`].
    pub version: Option<String>,
    pub source: InstallSource,
}

/// Progress of a pack operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PackEvent {
    Stage(String),
    Download {
        component: String,
        done: u64,
        total: u64,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InstallStatus {
    Installed,
    AlreadyInstalled,
}

/// Result of `ashift engines install`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InstallReport {
    pub name: String,
    pub version: String,
    pub target: String,
    pub status: InstallStatus,
    /// Minisign id of the key that signed the manifest.
    pub key_id: String,
    pub files: u64,
    pub bytes: u64,
    /// Install root; printed by the human output only.
    #[serde(skip)]
    pub root: PathBuf,
}

/// Result of `ashift engines remove`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RemoveReport {
    pub name: String,
    pub removed: Vec<String>,
    /// Active version after the removal, if any remains.
    pub active: Option<String>,
}

/// Result of `ashift engines verify`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VerifyReport {
    pub name: String,
    pub version: String,
    pub target: String,
    pub ok: bool,
    /// Key that signed the recorded manifest, when the signature still verifies.
    pub key_id: Option<String>,
    /// Files and links checked.
    pub checked: u64,
    pub modified: Vec<String>,
    pub missing: Vec<String>,
    /// Other failures (signature, receipt).
    pub problems: Vec<String>,
}

/// An installed pack version.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InstalledVersion {
    pub name: String,
    pub version: String,
    pub target: String,
    pub active: bool,
    pub installed_at: String,
    #[serde(skip)]
    pub root: PathBuf,
}

/// Quick health of an installed pack: receipt, file presence and sizes, link targets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PackHealth {
    Ok,
    Problem(String),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: String,
    name: String,
    version: String,
    target: String,
    installed_at: String,
    ashift_version: String,
    key_id: String,
    manifest: String,
    signature: String,
    #[serde(default)]
    files: Vec<ReceiptFile>,
    #[serde(default)]
    links: Vec<ReceiptLink>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptFile {
    path: String,
    sha256: String,
    size: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptLink {
    path: String,
    target: String,
}

struct Layout {
    packs: PathBuf,
}

impl Layout {
    fn new(context: &PackContext) -> Self {
        Self {
            packs: context.packs_dir(),
        }
    }
    fn locks(&self) -> PathBuf {
        self.packs.join(".locks")
    }
    fn staging(&self) -> PathBuf {
        self.packs.join(".staging")
    }
    fn cache(&self) -> PathBuf {
        self.packs.join(".cache")
    }
    fn pack_dir(&self, name: &str) -> PathBuf {
        self.packs.join(name)
    }
    fn version_dir(&self, name: &str, version: &str) -> PathBuf {
        self.packs.join(name).join(version)
    }
    fn current_file(&self, name: &str) -> PathBuf {
        self.packs.join(name).join(CURRENT_FILE)
    }
}

/// Deletes a staging file or directory unless disarmed.
struct Cleanup {
    path: PathBuf,
    armed: bool,
}

impl Cleanup {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if self.armed {
            remove_path(&self.path);
        }
    }
}

fn remove_path(path: &Path) {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => {
            let _ = fs::remove_dir_all(path);
        }
        Ok(_) => fetch::remove_file_quietly(path),
        Err(_) => {}
    }
}

/// Holds the per-pack lock; released when dropped.
struct PackLock {
    _file: File,
}

fn lock(layout: &Layout, name: &str) -> Result<PackLock, PackError> {
    let dir = layout.locks();
    create_dir_all(&dir)?;
    let path = dir.join(format!("{name}.lock"));
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|error| PackError::io("could not open", &path, error))?;
    match file.try_lock() {
        Ok(()) => Ok(PackLock { _file: file }),
        Err(TryLockError::WouldBlock) => Err(PackError::Busy {
            name: name.to_owned(),
        }),
        Err(TryLockError::Error(error)) => Err(PackError::io("could not lock", &path, error)),
    }
}

fn create_dir_all(path: &Path) -> Result<(), PackError> {
    fs::create_dir_all(path).map_err(|error| PackError::io("could not create", path, error))
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Downloads, verifies and installs a pack.
pub fn install(
    context: &PackContext,
    request: &InstallRequest,
    cancel: &CancellationToken,
    progress: &mut dyn FnMut(PackEvent),
) -> Result<InstallReport, PackError> {
    validate_pack_name(&request.name).map_err(PackError::Usage)?;
    if let Some(version) = &request.version {
        semver::Version::parse(version)
            .map_err(|error| PackError::Usage(format!("invalid version '{version}': {error}")))?;
    }
    let (manifest_location, requested_version, offline_dir) = match &request.source {
        InstallSource::Index => {
            let entry = index::lookup(&request.name)?;
            let version = request
                .version
                .clone()
                .unwrap_or_else(|| entry.version.clone());
            (
                Location::parse(&entry.manifest_url(&version))?,
                Some(version),
                None,
            )
        }
        InstallSource::Manifest(text) => (Location::parse(text)?, request.version.clone(), None),
        InstallSource::Directory(dir) => (
            Location::Path(dir.join(MANIFEST_FILE)),
            request.version.clone(),
            Some(dir.clone()),
        ),
    };
    let signature_location = match &offline_dir {
        Some(dir) => Location::Path(dir.join(SIGNATURE_FILE)),
        None => manifest_location.with_suffix(".minisig"),
    };

    let fetcher = Fetcher::new(context.fetch.clone());
    progress(PackEvent::Stage(format!(
        "fetch manifest {}",
        manifest_location.display()
    )));
    let manifest_bytes = fetcher.read_small(&manifest_location, MAX_MANIFEST_BYTES, cancel)?;
    let signature_bytes = fetcher.read_small(&signature_location, MAX_SIGNATURE_BYTES, cancel)?;
    let manifest_text = String::from_utf8(manifest_bytes)
        .map_err(|_| PackError::InvalidManifest("the manifest is not UTF-8".to_owned()))?;
    let signature_text =
        String::from_utf8(signature_bytes).map_err(|_| super::SignatureError::Malformed)?;

    let header = ManifestHeader::parse(&manifest_text)?;
    let key_id = context.keys.verify(
        manifest_text.as_bytes(),
        &signature_text,
        &header.name,
        &header.version,
    )?;
    if header.name != request.name {
        return Err(PackError::Mismatch {
            expected: format!("pack '{}'", request.name),
            found: format!("pack '{}'", header.name),
        });
    }
    if let Some(version) = &requested_version
        && &header.version != version
    {
        return Err(PackError::Mismatch {
            expected: format!("{} {version}", request.name),
            found: format!("{} {}", header.name, header.version),
        });
    }
    check_requires_ashift(&header.name, &header.version, &header.requires_ashift)?;
    let manifest = Manifest::parse(&manifest_text)?;
    let target = manifest
        .select_target(&context.targets)
        .ok_or_else(|| PackError::UnsupportedTarget {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            host: context.targets.join(", "),
            available: manifest.triples(),
        })?
        .clone();

    let layout = Layout::new(context);
    create_dir_all(&layout.packs)?;
    let _lock = lock(&layout, &manifest.name)?;
    sweep(&layout, &manifest.name);

    let final_dir = layout.version_dir(&manifest.name, &manifest.version);
    if let Ok(existing) = read_receipt(&final_dir, &manifest.name)
        && existing.manifest == manifest_text
        && existing.target == target.triple
        && quick_check(&final_dir, &existing).is_ok()
    {
        write_current(&layout, &manifest.name, &manifest.version)?;
        return Ok(InstallReport {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            target: target.triple.clone(),
            status: InstallStatus::AlreadyInstalled,
            key_id,
            files: existing.files.len() as u64,
            bytes: existing.files.iter().map(|file| file.size).sum(),
            root: final_dir,
        });
    }

    create_dir_all(&layout.cache())?;
    let mut cached = Vec::with_capacity(target.components.len());
    for component in &target.components {
        if cancel.is_cancelled() {
            return Err(PackError::Interrupted);
        }
        let source = match &offline_dir {
            Some(dir) => Location::Path(dir.join(url_file_name(&component.url)?)),
            None => manifest_location.resolve(&component.url)?,
        };
        cached.push(obtain(
            &layout, &fetcher, &source, component, cancel, progress,
        )?);
    }

    create_dir_all(&layout.staging())?;
    let staging = layout
        .staging()
        .join(format!("{}.{}", manifest.name, new_id()));
    fs::create_dir(&staging).map_err(|error| PackError::io("could not create", &staging, error))?;
    let mut staging_cleanup = Cleanup::new(staging.clone());

    let mut files: Vec<FileRecord> = Vec::new();
    let mut links: Vec<LinkRecord> = Vec::new();
    for (component, path) in target.components.iter().zip(&cached) {
        progress(PackEvent::Stage(format!("extract {}", component.name)));
        let extracted = extract::extract(
            path,
            component,
            &staging,
            Bounds::for_component(component),
            cancel,
        )?;
        files.extend(extracted.files);
        links.extend(extracted.links);
    }
    extract::check_links(&staging, &links)?;
    check_installed_paths(&staging, &manifest, &target)?;

    let receipt = Receipt {
        schema: RECEIPT_SCHEMA.to_owned(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        target: target.triple.clone(),
        installed_at: jiff::Timestamp::now().to_string(),
        ashift_version: env!("CARGO_PKG_VERSION").to_owned(),
        key_id: key_id.clone(),
        manifest: manifest_text,
        signature: signature_text,
        files: files
            .iter()
            .map(|file| ReceiptFile {
                path: file.path.clone(),
                sha256: file.sha256.clone(),
                size: file.size,
            })
            .collect(),
        links: links
            .iter()
            .map(|link| ReceiptLink {
                path: link.path.clone(),
                target: link.target.clone(),
            })
            .collect(),
    };
    let receipt_text = toml::to_string(&receipt).map_err(|error| PackError::InvalidReceipt {
        name: manifest.name.clone(),
        detail: error.to_string(),
    })?;
    let receipt_path = staging.join(RECEIPT_FILE);
    fs::write(&receipt_path, receipt_text)
        .map_err(|error| PackError::io("could not write", &receipt_path, error))?;

    if cancel.is_cancelled() {
        return Err(PackError::Interrupted);
    }
    progress(PackEvent::Stage("activate".to_owned()));
    create_dir_all(&layout.pack_dir(&manifest.name))?;
    let mut previous = None;
    if fs::symlink_metadata(&final_dir).is_ok() {
        let old = layout
            .staging()
            .join(format!("{}.{}.old", manifest.name, new_id()));
        fs::rename(&final_dir, &old)
            .map_err(|error| PackError::io("could not replace", &final_dir, error))?;
        previous = Some(Cleanup::new(old));
    }
    if let Err(error) = fs::rename(&staging, &final_dir) {
        if let Some(old) = previous.as_mut()
            && fs::rename(&old.path, &final_dir).is_ok()
        {
            old.disarm();
        }
        return Err(PackError::io(
            "could not move the pack into",
            &final_dir,
            error,
        ));
    }
    staging_cleanup.disarm();
    drop(previous);
    write_current(&layout, &manifest.name, &manifest.version)?;
    for path in &cached {
        fetch::remove_file_quietly(path);
    }

    Ok(InstallReport {
        name: manifest.name,
        version: manifest.version,
        target: target.triple,
        status: InstallStatus::Installed,
        key_id,
        files: files.len() as u64,
        bytes: files.iter().map(|file| file.size).sum(),
        root: final_dir,
    })
}

/// File name a component has in an offline `--from` directory: the last segment of its URL.
fn url_file_name(url: &str) -> Result<String, PackError> {
    let name = url
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .split(['?', '#'])
        .next()
        .unwrap_or_default();
    relative_path(name)
        .ok()
        .filter(|parts| parts.len() == 1)
        .map(|_| name.to_owned())
        .ok_or_else(|| {
            PackError::InvalidManifest(format!("component url '{url}' has no file name"))
        })
}

/// Returns a hash-verified copy of the component in the download cache.
fn obtain(
    layout: &Layout,
    fetcher: &Fetcher,
    source: &Location,
    component: &super::Component,
    cancel: &CancellationToken,
    progress: &mut dyn FnMut(PackEvent),
) -> Result<PathBuf, PackError> {
    let cached = layout.cache().join(&component.sha256);
    if cached.is_file() {
        progress(PackEvent::Stage(format!("check cached {}", component.name)));
        match fetch::sha256_file(&cached, cancel)? {
            (hash, size) if hash == component.sha256 && size == component.size => {
                return Ok(cached);
            }
            _ => fetch::remove_file_quietly(&cached),
        }
    }
    progress(PackEvent::Stage(format!(
        "download {} from {}",
        component.name,
        source.display()
    )));
    let part = layout
        .cache()
        .join(format!("{}.{}.part", component.sha256, new_id()));
    let mut part_cleanup = Cleanup::new(part.clone());
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part)
        .map_err(|error| PackError::io("could not create", &part, error))?;
    let name = component.name.clone();
    let total = component.size;
    fetcher.fetch_verified(
        source,
        &mut out,
        &part,
        &component.name,
        component.size,
        &component.sha256,
        cancel,
        &mut |done| {
            progress(PackEvent::Download {
                component: name.clone(),
                done,
                total,
            });
        },
    )?;
    drop(out);
    fs::rename(&part, &cached).map_err(|error| PackError::io("could not store", &cached, error))?;
    part_cleanup.disarm();
    Ok(cached)
}

/// Notices must be regular files and the entrypoint must exist inside the install root.
fn check_installed_paths(
    root: &Path,
    manifest: &Manifest,
    target: &super::Target,
) -> Result<(), PackError> {
    for notice in &manifest.notices {
        let path = extract::join_parts(root, notice);
        if !fs::metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
            return Err(PackError::InvalidManifest(format!(
                "notice '{notice}' is not a file in the pack"
            )));
        }
    }
    let program = &target.entrypoint[0];
    let path = extract::join_parts(root, program);
    if !fs::metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
        return Err(PackError::InvalidManifest(format!(
            "entrypoint '{program}' is not a file in the pack"
        )));
    }
    Ok(())
}

/// Removes leftovers of interrupted runs: this pack's staging directories, stale partial
/// downloads and old cached components. Staging names are `<name>.<id>`; `.` cannot occur in
/// a pack name, so the prefix never matches another pack.
fn sweep(layout: &Layout, name: &str) {
    let prefix = format!("{name}.");
    if let Ok(entries) = fs::read_dir(layout.staging()) {
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_str()
                .is_some_and(|file| file.starts_with(&prefix))
            {
                remove_path(&entry.path());
            }
        }
    }
    let now = SystemTime::now();
    if let Ok(entries) = fs::read_dir(layout.cache()) {
        for entry in entries.flatten() {
            let age = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .unwrap_or_default();
            let is_part = entry
                .file_name()
                .to_str()
                .is_some_and(|file| file.ends_with(".part"));
            if (is_part && age > STALE_PART_AGE) || age > STALE_CACHE_AGE {
                remove_path(&entry.path());
            }
        }
    }
}

fn write_current(layout: &Layout, name: &str, version: &str) -> Result<(), PackError> {
    let path = layout.current_file(name);
    let temporary = layout
        .pack_dir(name)
        .join(format!(".{CURRENT_FILE}.{}", new_id()));
    fs::write(&temporary, format!("{version}\n"))
        .map_err(|error| PackError::io("could not write", &temporary, error))?;
    fs::rename(&temporary, &path).map_err(|error| {
        fetch::remove_file_quietly(&temporary);
        PackError::io("could not write", &path, error)
    })
}

fn read_receipt(root: &Path, name: &str) -> Result<Receipt, PackError> {
    let path = root.join(RECEIPT_FILE);
    let text = fs::read_to_string(&path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            PackError::InvalidReceipt {
                name: name.to_owned(),
                detail: format!("{} is missing", path.display()),
            }
        } else {
            PackError::io("could not read", &path, error)
        }
    })?;
    let receipt: Receipt = toml::from_str(&text).map_err(|error| PackError::InvalidReceipt {
        name: name.to_owned(),
        detail: error.to_string(),
    })?;
    if receipt.schema != RECEIPT_SCHEMA || receipt.name != name {
        return Err(PackError::InvalidReceipt {
            name: name.to_owned(),
            detail: format!("{} does not describe this pack", path.display()),
        });
    }
    Ok(receipt)
}

/// Versions with a receipt, highest SemVer first.
fn installed_versions(layout: &Layout, name: &str) -> Vec<String> {
    let mut versions: Vec<semver::Version> = fs::read_dir(layout.pack_dir(name))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().join(RECEIPT_FILE).is_file())
        .filter_map(|entry| semver::Version::parse(entry.file_name().to_str()?).ok())
        .collect();
    versions.sort_by(|a, b| b.cmp(a));
    versions
        .into_iter()
        .map(|version| version.to_string())
        .collect()
}

fn active_version(layout: &Layout, name: &str) -> Option<String> {
    let versions = installed_versions(layout, name);
    let current = fs::read_to_string(layout.current_file(name))
        .ok()
        .map(|text| text.trim().to_owned());
    match current {
        Some(current) if versions.contains(&current) => Some(current),
        _ => versions.into_iter().next(),
    }
}

pub(crate) fn resolve_installed(
    context: &PackContext,
    name: &str,
) -> Result<Option<InstalledPack>, PackError> {
    let layout = Layout::new(context);
    let Some(version) = active_version(&layout, name) else {
        return Ok(None);
    };
    let root = layout.version_dir(name, &version);
    let receipt = read_receipt(&root, name)?;
    let manifest = Manifest::parse(&receipt.manifest)?;
    manifest.check_ashift()?;
    if !context.targets.contains(&receipt.target) {
        return Err(PackError::UnsupportedTarget {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            host: context.targets.join(", "),
            available: receipt.target.clone(),
        });
    }
    let target = manifest
        .targets
        .iter()
        .find(|target| target.triple == receipt.target)
        .ok_or_else(|| PackError::InvalidReceipt {
            name: name.to_owned(),
            detail: format!("the manifest has no target '{}'", receipt.target),
        })?;
    InstalledPack::from_manifest(&manifest, target, root, PackSource::Installed).map(Some)
}

/// Every installed pack version.
pub fn list_installed(context: &PackContext) -> Result<Vec<InstalledVersion>, PackError> {
    let layout = Layout::new(context);
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&layout.packs) else {
        return Ok(out);
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|name| validate_pack_name(name).is_ok())
        .collect();
    names.sort();
    for name in names {
        let active = active_version(&layout, &name);
        for version in installed_versions(&layout, &name) {
            let root = layout.version_dir(&name, &version);
            let Ok(receipt) = read_receipt(&root, &name) else {
                continue;
            };
            out.push(InstalledVersion {
                active: active.as_deref() == Some(version.as_str()),
                name: name.clone(),
                version,
                target: receipt.target,
                installed_at: receipt.installed_at,
                root,
            });
        }
    }
    Ok(out)
}

/// Quick check without hashing: receipt, file presence and sizes, link targets, and
/// compatibility with this `ashift`. Problems are described without paths.
pub fn check(context: &PackContext, name: &str, version: &str) -> PackHealth {
    let root = Layout::new(context).version_dir(name, version);
    let Ok(receipt) = read_receipt(&root, name) else {
        return PackHealth::Problem("the install receipt is missing or invalid".to_owned());
    };
    if quick_check(&root, &receipt).is_err() {
        return PackHealth::Problem("files are missing or modified".to_owned());
    }
    match Manifest::parse(&receipt.manifest).and_then(|manifest| manifest.check_ashift()) {
        Ok(()) if context.targets.contains(&receipt.target) => PackHealth::Ok,
        Ok(()) => PackHealth::Problem("the pack was built for another platform".to_owned()),
        Err(error) => PackHealth::Problem(error.to_string()),
    }
}

fn quick_check(root: &Path, receipt: &Receipt) -> Result<(), String> {
    for file in &receipt.files {
        let path = extract::join_parts(root, &file.path);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && metadata.len() == file.size => {}
            Ok(_) => return Err(format!("{} was modified", file.path)),
            Err(_) => return Err(format!("{} is missing", file.path)),
        }
    }
    for link in &receipt.links {
        let path = extract::join_parts(root, &link.path);
        match fs::read_link(&path) {
            Ok(target) if target == Path::new(&link.target) => {}
            Ok(_) => return Err(format!("{} was modified", link.path)),
            Err(_) => return Err(format!("{} is missing", link.path)),
        }
    }
    Ok(())
}

/// Re-hashes every recorded file and re-checks the manifest signature.
pub fn verify(
    context: &PackContext,
    name: &str,
    version: Option<&str>,
    cancel: &CancellationToken,
) -> Result<VerifyReport, PackError> {
    validate_pack_name(name).map_err(PackError::Usage)?;
    let layout = Layout::new(context);
    let version = select_version(&layout, name, version)?;
    let root = layout.version_dir(name, &version);
    let receipt = read_receipt(&root, name)?;
    let mut report = VerifyReport {
        name: name.to_owned(),
        version: version.clone(),
        target: receipt.target.clone(),
        ok: true,
        key_id: None,
        checked: 0,
        modified: Vec::new(),
        missing: Vec::new(),
        problems: Vec::new(),
    };
    match context.keys.verify(
        receipt.manifest.as_bytes(),
        &receipt.signature,
        &receipt.name,
        &receipt.version,
    ) {
        Ok(key_id) => report.key_id = Some(key_id),
        Err(error) => report.problems.push(format!("signature: {error}")),
    }
    if receipt.version != version {
        report
            .problems
            .push(format!("the receipt records version {}", receipt.version));
    }
    for file in &receipt.files {
        if cancel.is_cancelled() {
            return Err(PackError::Interrupted);
        }
        report.checked += 1;
        let path = extract::join_parts(&root, &file.path);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() => {
                let (hash, size) = fetch::sha256_file(&path, cancel)?;
                if hash != file.sha256 || size != file.size {
                    report.modified.push(file.path.clone());
                }
            }
            Ok(_) => report.modified.push(file.path.clone()),
            Err(_) => report.missing.push(file.path.clone()),
        }
    }
    for link in &receipt.links {
        report.checked += 1;
        let path = extract::join_parts(&root, &link.path);
        match fs::read_link(&path) {
            Ok(target) if target == Path::new(&link.target) => {}
            Ok(_) => report.modified.push(link.path.clone()),
            Err(_) => report.missing.push(link.path.clone()),
        }
    }
    report.ok =
        report.modified.is_empty() && report.missing.is_empty() && report.problems.is_empty();
    Ok(report)
}

fn select_version(layout: &Layout, name: &str, version: Option<&str>) -> Result<String, PackError> {
    let installed = installed_versions(layout, name);
    match version {
        Some(version) if installed.iter().any(|v| v == version) => Ok(version.to_owned()),
        Some(version) => Err(PackError::NotInstalled {
            name: name.to_owned(),
            version: Some(version.to_owned()),
        }),
        None => active_version(layout, name).ok_or_else(|| PackError::NotInstalled {
            name: name.to_owned(),
            version: None,
        }),
    }
}

/// Removes one version, or every version when `version` is `None`.
pub fn remove(
    context: &PackContext,
    name: &str,
    version: Option<&str>,
) -> Result<RemoveReport, PackError> {
    validate_pack_name(name).map_err(PackError::Usage)?;
    let layout = Layout::new(context);
    let installed = installed_versions(&layout, name);
    let targets: Vec<String> = match version {
        Some(version) if installed.iter().any(|v| v == version) => vec![version.to_owned()],
        Some(version) => {
            return Err(PackError::NotInstalled {
                name: name.to_owned(),
                version: Some(version.to_owned()),
            });
        }
        None if installed.is_empty() => {
            return Err(PackError::NotInstalled {
                name: name.to_owned(),
                version: None,
            });
        }
        None => installed.clone(),
    };
    let _lock = lock(&layout, name)?;
    create_dir_all(&layout.staging())?;
    for version in &targets {
        let dir = layout.version_dir(name, version);
        let trash = layout.staging().join(format!("{name}.{}.old", new_id()));
        fs::rename(&dir, &trash).map_err(|error| PackError::io("could not remove", &dir, error))?;
        let _ = fs::remove_dir_all(&trash);
    }
    let remaining: BTreeSet<&String> = installed.iter().filter(|v| !targets.contains(v)).collect();
    let active = if remaining.is_empty() {
        fetch::remove_file_quietly(&layout.current_file(name));
        let _ = fs::remove_dir(layout.pack_dir(name));
        None
    } else {
        let current = active_version(&layout, name);
        if let Some(current) = &current {
            write_current(&layout, name, current)?;
        }
        current
    };
    Ok(RemoveReport {
        name: name.to_owned(),
        removed: targets,
        active,
    })
}

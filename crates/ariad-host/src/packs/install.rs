//! Install, remove, verify and resolve packs under `<data>/packs`.
//!
//! Layout:
//!
//! ```text
//! <data>/packs/.locks/<name>.lock   exclusive while installing or removing <name>
//! <data>/packs/.staging/<name>.<id> extraction targets, renamed into place on success
//! <data>/packs/.cache/<name>.<sha256> downloaded, hash-verified components (resume)
//! <data>/packs/<name>/current       active version
//! <data>/packs/<name>/<version>/    install root with receipt.toml
//! ```

use std::{
    collections::{BTreeSet, HashMap},
    fs::{self, File, OpenOptions, TryLockError},
    io,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptFile {
    path: String,
    sha256: String,
    size: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    executable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptLink {
    path: String,
    target: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestRecord {
    manifest_sha256: String,
    target: String,
    #[serde(default)]
    files: Vec<ReceiptFile>,
    #[serde(default)]
    links: Vec<ReceiptLink>,
    #[serde(default)]
    executables: Vec<String>,
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
    fn manifest_record(&self, name: &str, version: &str) -> PathBuf {
        self.locks().join(format!("{name}-{version}.manifest"))
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
        && match verify(context, &manifest.name, Some(&manifest.version), cancel) {
            Ok(report) => report.ok,
            Err(PackError::Interrupted) => return Err(PackError::Interrupted),
            Err(_) => false,
        }
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
            &layout,
            &manifest.name,
            &fetcher,
            &source,
            component,
            cancel,
            progress,
        )?);
    }

    create_dir_all(&layout.staging())?;
    let staging = layout
        .staging()
        .join(format!("{}.{}", manifest.name, new_id()));
    fs::create_dir(&staging).map_err(|error| PackError::io("could not create", &staging, error))?;
    let mut staging_cleanup = Cleanup::new(staging.clone());

    let declared_executables = manifest_allowed_executables(&manifest, &target);
    let notice_paths: BTreeSet<String> = manifest
        .notices
        .iter()
        .filter_map(|notice| relative_path(notice).ok().map(|parts| parts.join("/")))
        .collect();
    let mut files: Vec<FileRecord> = Vec::new();
    let mut links: Vec<LinkRecord> = Vec::new();
    for (component, path) in target.components.iter().zip(&cached) {
        progress(PackEvent::Stage(format!("extract {}", component.name)));
        let mut extracted = extract::extract(
            path,
            component,
            &staging,
            Bounds::for_component(component),
            cancel,
        )?;
        for file in &mut extracted.files {
            file.executable = if component.unpack.is_archive() {
                let extracted_path = extract::join_parts(&staging, &file.path);
                (file.executable
                    || fs::symlink_metadata(&extracted_path)
                        .is_ok_and(|metadata| is_executable_file(&extracted_path, &metadata)))
                    && !notice_paths.contains(&file.path)
                    && declared_executables.contains(&file.path)
            } else {
                component.executable || declared_executables.contains(&file.path)
            };
        }
        files.extend(extracted.files);
        links.extend(extracted.links);
    }
    extract::check_links(&staging, &links)?;
    check_installed_paths(&staging, &manifest, &target)?;

    let manifest_executables: Vec<String> = declared_executables.into_iter().collect();

    let receipt_files: Vec<ReceiptFile> = files
        .iter()
        .map(|file| ReceiptFile {
            path: file.path.clone(),
            sha256: file.sha256.clone(),
            size: file.size,
            executable: file.executable,
        })
        .collect();
    let receipt_links: Vec<ReceiptLink> = links
        .iter()
        .map(|link| ReceiptLink {
            path: link.path.clone(),
            target: link.target.clone(),
        })
        .collect();
    let receipt = Receipt {
        schema: RECEIPT_SCHEMA.to_owned(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        target: target.triple.clone(),
        installed_at: jiff::Timestamp::now().to_string(),
        ashift_version: env!("CARGO_PKG_VERSION").to_owned(),
        key_id: key_id.clone(),
        manifest: manifest_text.clone(),
        signature: signature_text,
        files: receipt_files.clone(),
        links: receipt_links.clone(),
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
    write_manifest_record(
        &layout,
        &manifest.name,
        &manifest.version,
        &manifest_text,
        &target.triple,
        &receipt_files,
        &receipt_links,
        &manifest_executables,
    )?;
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
    pack: &str,
    fetcher: &Fetcher,
    source: &Location,
    component: &super::Component,
    cancel: &CancellationToken,
    progress: &mut dyn FnMut(PackEvent),
) -> Result<PathBuf, PackError> {
    let cached = layout.cache().join(format!("{pack}.{}", component.sha256));
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
        .join(format!("{pack}.{}.{}.part", component.sha256, new_id()));
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

fn sha256_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

#[allow(clippy::too_many_arguments)]
fn write_manifest_record(
    layout: &Layout,
    name: &str,
    version: &str,
    manifest_text: &str,
    target: &str,
    files: &[ReceiptFile],
    links: &[ReceiptLink],
    executables: &[String],
) -> Result<(), PackError> {
    create_dir_all(&layout.locks())?;
    let record = ManifestRecord {
        manifest_sha256: sha256_bytes(manifest_text.as_bytes()),
        target: target.to_owned(),
        files: files.to_vec(),
        links: links.to_vec(),
        executables: executables.to_vec(),
    };
    let text = toml::to_string(&record).map_err(|error| PackError::InvalidReceipt {
        name: name.to_owned(),
        detail: error.to_string(),
    })?;
    let path = layout.manifest_record(name, version);
    let temporary = layout
        .locks()
        .join(format!(".{name}-{version}.manifest.{}", new_id()));
    fs::write(&temporary, text)
        .map_err(|error| PackError::io("could not write", &temporary, error))?;
    fs::rename(&temporary, &path).map_err(|error| {
        fetch::remove_file_quietly(&temporary);
        PackError::io("could not write", &path, error)
    })
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

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct FileStamp {
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
    #[cfg(unix)]
    ctime_sec: i64,
    #[cfg(unix)]
    ctime_nsec: i64,
    mtime_sec: u64,
    mtime_nsec: u32,
    size: u64,
    file_count: u64,
}

impl FileStamp {
    fn from_metadata(metadata: &fs::Metadata, file_count: u64) -> Option<Self> {
        let modified = metadata.modified().ok()?;
        let since_epoch = modified.duration_since(UNIX_EPOCH).ok()?;
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Some(Self {
            #[cfg(unix)]
            dev: metadata.dev(),
            #[cfg(unix)]
            ino: metadata.ino(),
            #[cfg(unix)]
            ctime_sec: metadata.ctime(),
            #[cfg(unix)]
            ctime_nsec: metadata.ctime_nsec(),
            mtime_sec: since_epoch.as_secs(),
            mtime_nsec: since_epoch.subsec_nanos(),
            size: metadata.len(),
            file_count,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CachedDigest {
    stamp: FileStamp,
    sha256: String,
}

static VERIFY_CACHE: LazyLock<Mutex<HashMap<PathBuf, CachedDigest>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn verified_sha256(
    path: &Path,
    metadata: &fs::Metadata,
    file_count: u64,
) -> Result<String, PackError> {
    let stamp = FileStamp::from_metadata(metadata, file_count);
    if let Some(ref stamp) = stamp {
        let cache = VERIFY_CACHE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entry) = cache.get(path)
            && &entry.stamp == stamp
        {
            return Ok(entry.sha256.clone());
        }
    }
    let cancel = CancellationToken::new();
    let (digest, _) = fetch::sha256_file(path, &cancel)?;
    crate::engines::resolve::record_hash_computation(path);
    if let Some(stamp) = stamp {
        let mut cache = VERIFY_CACHE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.insert(
            path.to_path_buf(),
            CachedDigest {
                stamp,
                sha256: digest.clone(),
            },
        );
    }
    Ok(digest)
}

#[derive(Debug, Default)]
struct PackDirScan {
    file_count: u64,
    found_files: BTreeSet<String>,
    found_links: BTreeSet<String>,
    found_executables: BTreeSet<String>,
}

fn is_executable_file(path: &Path, metadata: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = path;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        path.extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                ext.eq_ignore_ascii_case("exe")
                    || ext.eq_ignore_ascii_case("bat")
                    || ext.eq_ignore_ascii_case("cmd")
            })
    }
}

fn scan_pack_dir(root: &Path, name: &str, version: &str) -> Result<PackDirScan, PackError> {
    fn walk(
        root: &Path,
        dir: &Path,
        name: &str,
        version: &str,
        scan: &mut PackDirScan,
    ) -> Result<(), PackError> {
        let entries = fs::read_dir(dir).map_err(|error| {
            verification_failed(
                name,
                version,
                format!("could not read pack directory {}: {error}", dir.display()),
            )
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                verification_failed(
                    name,
                    version,
                    format!("could not read entry in {}: {error}", dir.display()),
                )
            })?;
            let path = entry.path();
            let rel_os = path.strip_prefix(root).map_err(|error| {
                verification_failed(name, version, format!("invalid path in pack root: {error}"))
            })?;
            let rel = rel_os.to_string_lossy().replace('\\', "/");
            if dir == root && matches!(rel.as_str(), RECEIPT_FILE | MANIFEST_FILE | SIGNATURE_FILE)
            {
                continue;
            }
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                verification_failed(name, version, format!("could not stat '{rel}': {error}"))
            })?;
            let file_type = metadata.file_type();
            if file_type.is_dir() {
                walk(root, &path, name, version, scan)?;
            } else if file_type.is_symlink() {
                scan.found_links.insert(rel);
            } else if file_type.is_file() {
                scan.file_count += 1;
                if is_executable_file(&path, &metadata) {
                    scan.found_executables.insert(rel.clone());
                }
                scan.found_files.insert(rel);
            } else {
                return Err(verification_failed(
                    name,
                    version,
                    format!("unexpected special file '{rel}' in pack directory"),
                ));
            }
        }
        Ok(())
    }

    let mut scan = PackDirScan::default();
    walk(root, root, name, version, &mut scan)?;
    Ok(scan)
}

fn manifest_allowed_executables(manifest: &Manifest, target: &super::Target) -> BTreeSet<String> {
    let notice_paths: BTreeSet<String> = manifest
        .notices
        .iter()
        .filter_map(|notice| relative_path(notice).ok().map(|parts| parts.join("/")))
        .collect();
    let entrypoint_path = target
        .entrypoint
        .first()
        .and_then(|entrypoint| relative_path(entrypoint).ok().map(|parts| parts.join("/")));
    let mut allowed = BTreeSet::new();
    if let Some(ref entrypoint) = entrypoint_path {
        allowed.insert(entrypoint.clone());
    }
    for exec in &target.executables {
        if let Ok(parts) = relative_path(exec) {
            let rel = parts.join("/");
            if !notice_paths.contains(&rel) || entrypoint_path.as_deref() == Some(rel.as_str()) {
                allowed.insert(rel);
            }
        }
    }
    for component in &target.components {
        if component.unpack == super::Unpack::File
            && let Ok(parts) = relative_path(&component.file_path())
        {
            let rel = parts.join("/");
            if component.executable {
                allowed.insert(rel);
            } else if entrypoint_path.as_deref() != Some(rel.as_str()) {
                allowed.remove(&rel);
            }
        }
    }
    allowed
}

struct VerifiedManifestEntries {
    files: Vec<ReceiptFile>,
    links: Vec<ReceiptLink>,
    allowed_executables: BTreeSet<String>,
}

fn manifest_verified_entries(
    layout: &Layout,
    name: &str,
    version: &str,
    manifest: &Manifest,
    target: &super::Target,
    receipt: &Receipt,
) -> Result<VerifiedManifestEntries, PackError> {
    let record_path = layout.manifest_record(name, version);
    let record_text = fs::read_to_string(&record_path).map_err(|_| {
        verification_failed(name, version, "manifest file digest record is missing")
    })?;
    let record: ManifestRecord = toml::from_str(&record_text).map_err(|error| {
        verification_failed(
            name,
            version,
            format!("manifest file digest record is invalid: {error}"),
        )
    })?;
    let expected_manifest_sha256 = sha256_bytes(receipt.manifest.as_bytes());
    if record.manifest_sha256 != expected_manifest_sha256 || record.target != target.triple {
        return Err(verification_failed(
            name,
            version,
            "manifest file digest record does not match the signed manifest",
        ));
    }

    // Resolve grants execution only to paths declared in the signed manifest (`entrypoint[0]`,
    // `target.executables`) plus `unpack = "file"` components with `executable = true`.
    // The receipt and local manifest record `executable`/`executables` fields stay for
    // diagnostics only and never widen the allowed executable set.
    let declared_executables = manifest_allowed_executables(manifest, target);
    let mut allowed_executables = declared_executables.clone();

    let mut by_path: HashMap<&str, (&str, u64, bool)> = record
        .files
        .iter()
        .map(|file| {
            (
                file.path.as_str(),
                (file.sha256.as_str(), file.size, file.executable),
            )
        })
        .collect();
    for component in &target.components {
        if component.unpack == super::Unpack::File {
            let rel = relative_path(&component.file_path())
                .map(|parts| parts.join("/"))
                .map_err(|error| verification_failed(name, version, error))?;
            match by_path.get(rel.as_str()) {
                Some(&(sha256, size, executable))
                    if sha256 == component.sha256
                        && size == component.size
                        && (!executable
                            || component.executable
                            || declared_executables.contains(&rel)) => {}
                _ => {
                    return Err(verification_failed(
                        name,
                        version,
                        format!(
                            "file '{}' does not match signed manifest component '{}'",
                            rel, component.name
                        ),
                    ));
                }
            }
            if !component.executable && !declared_executables.contains(&rel) {
                allowed_executables.remove(&rel);
            }
            by_path.remove(rel.as_str());
        }
    }

    let expected_total_bytes: u64 = target
        .components
        .iter()
        .map(|component| component.unpacked_size.unwrap_or(component.size))
        .sum();
    let recorded_total_bytes: u64 = record.files.iter().map(|file| file.size).sum();
    if recorded_total_bytes != expected_total_bytes {
        return Err(verification_failed(
            name,
            version,
            "recorded file byte total does not match signed manifest unpacked_size",
        ));
    }

    Ok(VerifiedManifestEntries {
        files: record.files,
        links: record.links,
        allowed_executables,
    })
}

fn check_receipt_matches_manifest_record(
    name: &str,
    version: &str,
    receipt: &Receipt,
    expected_files: &[ReceiptFile],
    expected_links: &[ReceiptLink],
) -> Result<(), PackError> {
    if receipt.files != expected_files || receipt.links != expected_links {
        return Err(verification_failed(
            name,
            version,
            "unsigned receipt file table does not match the signed manifest",
        ));
    }
    Ok(())
}

fn any_version_directory(layout: &Layout, name: &str) -> Option<String> {
    let mut versions: Vec<semver::Version> = fs::read_dir(layout.pack_dir(name))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| semver::Version::parse(entry.file_name().to_str()?).ok())
        .collect();
    versions.sort_by(|a, b| b.cmp(a));
    versions.into_iter().next().map(|v| v.to_string())
}

fn verification_failed(name: &str, version: &str, reason: impl std::fmt::Display) -> PackError {
    PackError::VerificationFailed {
        name: name.to_owned(),
        version: version.to_owned(),
        detail: format!("tool_missing: {reason}; run 'ashift engines install {name}' to reinstall"),
    }
}

pub(crate) fn resolve_installed(
    context: &PackContext,
    name: &str,
) -> Result<Option<InstalledPack>, PackError> {
    let layout = Layout::new(context);
    let Some(version) =
        active_version(&layout, name).or_else(|| any_version_directory(&layout, name))
    else {
        return Ok(None);
    };
    let root = layout.version_dir(name, &version);
    let receipt =
        read_receipt(&root, name).map_err(|error| verification_failed(name, &version, error))?;
    if receipt.version != version {
        return Err(verification_failed(
            name,
            &version,
            format!(
                "receipt version '{}' does not match installed directory '{version}'",
                receipt.version
            ),
        ));
    }
    if root.join(MANIFEST_FILE).exists()
        && fs::read_to_string(root.join(MANIFEST_FILE)).ok().as_deref()
            != Some(receipt.manifest.as_str())
    {
        return Err(verification_failed(
            name,
            &version,
            "pack.toml on disk does not match the signed receipt manifest",
        ));
    }
    if root.join(SIGNATURE_FILE).exists()
        && fs::read_to_string(root.join(SIGNATURE_FILE))
            .ok()
            .as_deref()
            != Some(receipt.signature.as_str())
    {
        return Err(verification_failed(
            name,
            &version,
            "pack.toml.minisig on disk does not match the signed receipt signature",
        ));
    }
    context
        .keys
        .verify(
            receipt.manifest.as_bytes(),
            &receipt.signature,
            &receipt.name,
            &receipt.version,
        )
        .map_err(|error| verification_failed(name, &version, error))?;
    let manifest = Manifest::parse(&receipt.manifest)
        .map_err(|error| verification_failed(name, &version, error))?;
    if manifest.name != name || manifest.version != version {
        return Err(verification_failed(
            name,
            &version,
            "signed manifest identity does not match installed pack",
        ));
    }
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
        .ok_or_else(|| {
            verification_failed(
                name,
                &version,
                format!("the manifest has no target '{}'", receipt.target),
            )
        })?;
    let scan = scan_pack_dir(&root, name, &version)?;
    let VerifiedManifestEntries {
        files: expected_files,
        links: expected_links,
        allowed_executables,
    } = manifest_verified_entries(&layout, name, &version, &manifest, target, &receipt)?;
    for exec_path in &scan.found_executables {
        if !allowed_executables.contains(exec_path) {
            return Err(verification_failed(
                name,
                &version,
                format!(
                    "unlisted executable '{exec_path}' in pack directory is not declared in the signed manifest"
                ),
            ));
        }
    }
    check_receipt_matches_manifest_record(
        name,
        &version,
        &receipt,
        &expected_files,
        &expected_links,
    )?;
    for link in &expected_links {
        let path = extract::join_parts(&root, &link.path);
        match fs::read_link(&path) {
            Ok(actual) if actual == Path::new(&link.target) => {}
            Ok(_) => {
                return Err(verification_failed(
                    name,
                    &version,
                    format!("symlink '{}' was modified", link.path),
                ));
            }
            Err(_) => {
                return Err(verification_failed(
                    name,
                    &version,
                    format!("symlink '{}' is missing", link.path),
                ));
            }
        }
    }
    for file in &expected_files {
        let path = extract::join_parts(&root, &file.path);
        let metadata = fs::symlink_metadata(&path).map_err(|_| {
            verification_failed(name, &version, format!("file '{}' is missing", file.path))
        })?;
        if !metadata.is_file() || metadata.len() != file.size {
            return Err(verification_failed(
                name,
                &version,
                format!("file '{}' size or type was modified", file.path),
            ));
        }
        let actual_sha256 = verified_sha256(&path, &metadata, scan.file_count)?;
        if actual_sha256 != file.sha256 {
            return Err(verification_failed(
                name,
                &version,
                format!(
                    "SHA-256 mismatch for '{}' (expected {}, got {actual_sha256})",
                    file.path, file.sha256
                ),
            ));
        }
    }
    let expected_file_paths: BTreeSet<String> = expected_files
        .iter()
        .map(|file| file.path.clone())
        .collect();
    if scan.found_files != expected_file_paths {
        return Err(verification_failed(
            name,
            &version,
            "installed files in pack directory do not match the signed manifest",
        ));
    }
    let expected_link_paths: BTreeSet<String> = expected_links
        .iter()
        .map(|link| link.path.clone())
        .collect();
    if scan.found_links != expected_link_paths {
        return Err(verification_failed(
            name,
            &version,
            "installed symlinks in pack directory do not match the signed manifest",
        ));
    }
    InstalledPack::from_manifest(&manifest, target, root, PackSource::Installed)
        .map(Some)
        .map_err(|error| verification_failed(name, &version, error))
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

/// Quick check without hashing: signature, receipt, file presence and sizes, link targets, and
/// compatibility with this `ashift`. Problems are described without paths.
pub fn check(context: &PackContext, name: &str, version: &str) -> PackHealth {
    let root = Layout::new(context).version_dir(name, version);
    let Ok(receipt) = read_receipt(&root, name) else {
        return PackHealth::Problem("the install receipt is missing or invalid".to_owned());
    };
    if context
        .keys
        .verify(
            receipt.manifest.as_bytes(),
            &receipt.signature,
            &receipt.name,
            &receipt.version,
        )
        .is_err()
    {
        return PackHealth::Problem("the manifest signature is invalid or untrusted".to_owned());
    }
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
    if let Ok(manifest) = Manifest::parse(&receipt.manifest)
        && let Some(target) = manifest
            .targets
            .iter()
            .find(|target| target.triple == receipt.target)
        && let Err(error) =
            manifest_verified_entries(&layout, name, &version, &manifest, target, &receipt)
                .and_then(|verified| {
                    check_receipt_matches_manifest_record(
                        name,
                        &version,
                        &receipt,
                        &verified.files,
                        &verified.links,
                    )
                })
    {
        report.problems.push(error.to_string());
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
        fetch::remove_file_quietly(&layout.manifest_record(name, version));
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

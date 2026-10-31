//! Safe extraction of pack components.
//!
//! Rules for every archive entry: the name is relative, UTF-8 and free of `..`, backslashes,
//! `:` and control characters; only regular files, directories, symbolic links and hard links
//! are accepted; no entry is written through an existing symbolic link or file; symbolic link
//! targets are relative and stay inside the install root, both lexically and after
//! canonicalization ([`check_links`]); the entry count and the unpacked bytes stay within the
//! manifest's declared values plus a margin.

use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{self, BufReader, Read, Write},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use super::{
    PackError,
    manifest::{Component, RESERVED_ROOT_NAMES, Unpack, relative_path},
};

const COPY_BUFFER_BYTES: usize = 256 * 1024;
const MAX_LINK_TARGET_BYTES: u64 = 4096;
/// Tar framing allowance per entry: one header, PAX or GNU name records and padding.
const TAR_OVERHEAD_PER_ENTRY: u64 = 4 * 1024;

/// Upper bounds for one component.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bounds {
    pub max_bytes: u64,
    pub max_entries: u64,
}

impl Bounds {
    /// The declared sizes plus a margin of 1 % and 1 MiB or 16 entries.
    pub(crate) fn for_component(component: &Component) -> Self {
        let bytes = component.unpacked_size.unwrap_or(component.size);
        let entries = component.entries.unwrap_or(1);
        Self {
            max_bytes: bytes.saturating_add(bytes / 100).saturating_add(1 << 20),
            max_entries: entries.saturating_add(entries / 100).saturating_add(16),
        }
    }

    pub(crate) const fn unbounded() -> Self {
        Self {
            max_bytes: u64::MAX,
            max_entries: u64::MAX,
        }
    }
}

/// What an archive holds, counted the way [`Component::unpacked_size`] and
/// [`Component::entries`] are declared.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Stats {
    /// Bytes of regular files, hard-link copies included.
    pub bytes: u64,
    /// Files, directories and links listed in the archive.
    pub entries: u64,
}

/// A regular file written by extraction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FileRecord {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

/// A symbolic link written by extraction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LinkRecord {
    pub component: String,
    pub path: String,
    pub target: String,
}

#[derive(Debug, Default)]
pub(crate) struct Extracted {
    pub files: Vec<FileRecord>,
    pub links: Vec<LinkRecord>,
}

enum Kind {
    File { executable: bool },
    Dir,
    Symlink(String),
    HardLink(Vec<String>),
}

struct Budget<'a> {
    component: &'a str,
    bounds: Bounds,
    stats: Stats,
}

impl Budget<'_> {
    fn count_entry(&mut self) -> Result<(), PackError> {
        self.stats.entries += 1;
        if self.stats.entries > self.bounds.max_entries {
            return Err(PackError::ExtractLimit {
                component: self.component.to_owned(),
                what: "entry count",
                limit: self.bounds.max_entries,
            });
        }
        Ok(())
    }

    fn count_bytes(&mut self, bytes: u64) -> Result<(), PackError> {
        self.stats.bytes = self.stats.bytes.saturating_add(bytes);
        if self.stats.bytes > self.bounds.max_bytes {
            return Err(PackError::ExtractLimit {
                component: self.component.to_owned(),
                what: "unpacked size",
                limit: self.bounds.max_bytes,
            });
        }
        Ok(())
    }
}

trait Sink {
    fn dir(&mut self, full: &[String]) -> Result<(), PackError>;
    fn file(
        &mut self,
        full: &[String],
        reader: &mut dyn Read,
        executable: bool,
        budget: &mut Budget<'_>,
    ) -> Result<(), PackError>;
    fn symlink(&mut self, full: &[String], target: &str) -> Result<(), PackError>;
    fn hardlink(
        &mut self,
        full: &[String],
        target: &[String],
        budget: &mut Budget<'_>,
    ) -> Result<(), PackError>;
}

/// Extracts `component` from the verified file at `source` into `root`.
pub(crate) fn extract(
    source: &Path,
    component: &Component,
    root: &Path,
    bounds: Bounds,
    cancel: &CancellationToken,
) -> Result<Extracted, PackError> {
    let mut sink = Writer {
        root,
        component: &component.name,
        files: Vec::new(),
        file_index: HashMap::new(),
        links: Vec::new(),
        cancel,
    };
    let stats = walk(
        source,
        component.unpack,
        &component.name,
        &component.dest,
        component.executable,
        bounds,
        &mut sink,
        cancel,
    )?;
    if component
        .entries
        .is_some_and(|entries| entries != stats.entries)
        || component
            .unpacked_size
            .is_some_and(|bytes| bytes != stats.bytes)
    {
        return Err(corrupt(
            &component.name,
            format!(
                "decoded {} entries and {} bytes, which do not match the manifest",
                stats.entries, stats.bytes
            ),
        ));
    }
    Ok(Extracted {
        files: sink.files,
        links: sink.links,
    })
}

/// Reads an archive with the extraction rules but writes nothing; used to declare
/// `unpacked_size` and `entries` when a manifest is generated.
pub fn scan(source: &Path, unpack: Unpack, component: &str) -> Result<Stats, PackError> {
    let mut sink = Scanner {
        component: component.to_owned(),
        ..Scanner::default()
    };
    walk(
        source,
        unpack,
        component,
        ".",
        false,
        Bounds::unbounded(),
        &mut sink,
        &CancellationToken::new(),
    )
}

/// Runs the install-time archive writer, budgets and final link check in an existing,
/// caller-owned isolated `root`. Public only for fuzzing and confinement regression tests.
/// This bypasses manifest signature and component hash verification; real installs use
/// `install`, which performs those checks before extraction.
#[doc(hidden)]
pub fn extract_for_fuzzing(
    source: &Path,
    unpack: Unpack,
    component: &str,
    root: &Path,
    max_bytes: u64,
    max_entries: u64,
) -> Result<Stats, PackError> {
    let cancel = CancellationToken::new();
    let mut sink = Writer {
        root,
        component,
        files: Vec::new(),
        file_index: HashMap::new(),
        links: Vec::new(),
        cancel: &cancel,
    };
    let stats = walk(
        source,
        unpack,
        component,
        ".",
        false,
        Bounds {
            max_bytes,
            max_entries,
        },
        &mut sink,
        &cancel,
    )?;
    check_links(root, &sink.links)?;
    Ok(stats)
}

#[allow(clippy::too_many_arguments)]
fn walk(
    source: &Path,
    unpack: Unpack,
    component: &str,
    dest: &str,
    executable: bool,
    bounds: Bounds,
    sink: &mut dyn Sink,
    cancel: &CancellationToken,
) -> Result<Stats, PackError> {
    let dest_parts: Vec<String> = if dest == "." {
        Vec::new()
    } else {
        relative_path(dest)
            .map_err(|reason| unsafe_entry(component, dest, reason))?
            .into_iter()
            .map(str::to_owned)
            .collect()
    };
    let mut budget = Budget {
        component,
        bounds,
        stats: Stats::default(),
    };
    let file =
        File::open(source).map_err(|error| PackError::io("could not open", source, error))?;
    match unpack {
        Unpack::File => {
            let mut full = dest_parts;
            full.push(component.to_owned());
            check_reserved(component, &full)?;
            budget.count_entry()?;
            let mut reader = BufReader::new(file);
            sink.file(&full, &mut reader, executable, &mut budget)?;
        }
        Unpack::TarZst => {
            let decoder = super::zstd::Decoder::new(BufReader::new(file));
            walk_tar(decoder, component, &dest_parts, &mut budget, sink, cancel)?;
        }
        Unpack::TarGz => {
            let decoder = flate2::read::MultiGzDecoder::new(BufReader::new(file));
            walk_tar(decoder, component, &dest_parts, &mut budget, sink, cancel)?;
        }
        Unpack::Zip => walk_zip(file, component, &dest_parts, &mut budget, sink, cancel)?,
    }
    Ok(budget.stats)
}

fn walk_tar<R: Read>(
    reader: R,
    component: &str,
    dest: &[String],
    budget: &mut Budget<'_>,
    sink: &mut dyn Sink,
    cancel: &CancellationToken,
) -> Result<(), PackError> {
    let stream_limit = budget.bounds.max_bytes.saturating_add(
        budget
            .bounds
            .max_entries
            .saturating_add(16)
            .saturating_mul(TAR_OVERHEAD_PER_ENTRY),
    );
    let limited = LimitedReader {
        inner: reader,
        remaining: stream_limit,
    };
    let mut archive = tar::Archive::new(limited);
    let entries = archive
        .entries()
        .map_err(|error| tar_error(component, budget, error))?;
    for entry in entries {
        if cancel.is_cancelled() {
            return Err(PackError::Interrupted);
        }
        let mut entry = entry.map_err(|error| tar_error(component, budget, error))?;
        let raw_name = entry.path_bytes().into_owned();
        let display = String::from_utf8_lossy(&raw_name).into_owned();
        let header = entry.header();
        let kind = match header.entry_type() {
            tar::EntryType::Regular | tar::EntryType::Continuous => Kind::File {
                executable: header.mode().is_ok_and(|mode| mode & 0o111 != 0),
            },
            tar::EntryType::Directory => Kind::Dir,
            tar::EntryType::Symlink => {
                let target = link_name(&entry, component, &display)?;
                Kind::Symlink(target)
            }
            tar::EntryType::Link => {
                let target = link_name(&entry, component, &display)?;
                let parts =
                    entry_parts(component, &target, target.as_bytes())?.ok_or_else(|| {
                        unsafe_entry(component, &display, "hard link to the archive root")
                    })?;
                Kind::HardLink(prefixed(dest, parts))
            }
            tar::EntryType::XGlobalHeader => continue,
            _ => {
                return Err(unsafe_entry(
                    component,
                    &display,
                    "device, FIFO, sparse or other special entry",
                ));
            }
        };
        let Some(parts) = entry_parts(component, &display, &raw_name)? else {
            if matches!(kind, Kind::Dir) {
                continue;
            }
            return Err(unsafe_entry(component, &display, "entry has no name"));
        };
        let full = prefixed(dest, parts);
        apply(component, &display, &full, kind, &mut entry, budget, sink)?;
    }
    Ok(())
}

fn walk_zip(
    file: File,
    component: &str,
    dest: &[String],
    budget: &mut Budget<'_>,
    sink: &mut dyn Sink,
    cancel: &CancellationToken,
) -> Result<(), PackError> {
    let mut archive = zip::ZipArchive::new(file).map_err(|error| corrupt(component, error))?;
    if archive.len() as u64 > budget.bounds.max_entries {
        return Err(PackError::ExtractLimit {
            component: component.to_owned(),
            what: "entry count",
            limit: budget.bounds.max_entries,
        });
    }
    for index in 0..archive.len() {
        if cancel.is_cancelled() {
            return Err(PackError::Interrupted);
        }
        let mut entry = archive
            .by_index(index)
            .map_err(|error| corrupt(component, error))?;
        let raw_name = entry.name_raw().to_vec();
        let display = String::from_utf8_lossy(&raw_name).into_owned();
        if entry.encrypted() {
            return Err(unsafe_entry(component, &display, "entry is encrypted"));
        }
        let kind = match entry.unix_mode().map(|mode| mode & 0o170_000) {
            _ if entry.is_dir() => Kind::Dir,
            None | Some(0 | 0o100_000) => Kind::File {
                executable: entry.unix_mode().is_some_and(|mode| mode & 0o111 != 0),
            },
            Some(0o040_000) => Kind::Dir,
            Some(0o120_000) => {
                let mut target = Vec::new();
                (&mut entry)
                    .take(MAX_LINK_TARGET_BYTES + 1)
                    .read_to_end(&mut target)
                    .map_err(|error| corrupt(component, error))?;
                if target.len() as u64 > MAX_LINK_TARGET_BYTES {
                    return Err(unsafe_entry(component, &display, "link target is too long"));
                }
                let target = String::from_utf8(target)
                    .map_err(|_| unsafe_entry(component, &display, "link target is not UTF-8"))?;
                Kind::Symlink(target)
            }
            Some(_) => {
                return Err(unsafe_entry(
                    component,
                    &display,
                    "device, FIFO or other special entry",
                ));
            }
        };
        let Some(parts) = entry_parts(component, &display, &raw_name)? else {
            if matches!(kind, Kind::Dir) {
                continue;
            }
            return Err(unsafe_entry(component, &display, "entry has no name"));
        };
        let full = prefixed(dest, parts);
        apply(component, &display, &full, kind, &mut entry, budget, sink)?;
    }
    Ok(())
}

fn apply(
    component: &str,
    display: &str,
    full: &[String],
    kind: Kind,
    reader: &mut dyn Read,
    budget: &mut Budget<'_>,
    sink: &mut dyn Sink,
) -> Result<(), PackError> {
    check_reserved(component, full)?;
    budget.count_entry()?;
    match kind {
        Kind::File { executable } => sink.file(full, reader, executable, budget),
        Kind::Dir => sink.dir(full),
        Kind::Symlink(target) => {
            check_link_target(component, display, full, &target)?;
            sink.symlink(full, &target)
        }
        Kind::HardLink(target) => sink.hardlink(full, &target, budget),
    }
}

/// Normalizes an archive entry name into path components. Leading `./` and repeated or
/// trailing slashes are dropped; `None` means the entry names the archive root.
fn entry_parts(
    component: &str,
    display: &str,
    raw: &[u8],
) -> Result<Option<Vec<String>>, PackError> {
    let name = std::str::from_utf8(raw)
        .map_err(|_| unsafe_entry(component, display, "name is not UTF-8"))?;
    if name.starts_with('/') {
        return Err(unsafe_entry(component, display, "absolute path"));
    }
    if name
        .chars()
        .any(|c| c == '\\' || c == ':' || c.is_control())
    {
        return Err(unsafe_entry(
            component,
            display,
            "name contains a backslash, ':' or a control character",
        ));
    }
    let mut parts = Vec::new();
    for part in name.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(unsafe_entry(component, display, "path contains '..'")),
            part => {
                check_portable_component(component, display, part)?;
                parts.push(part.to_owned());
            }
        }
    }
    Ok((!parts.is_empty()).then_some(parts))
}

#[cfg(windows)]
fn check_portable_component(component: &str, display: &str, part: &str) -> Result<(), PackError> {
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = part.split('.').next().unwrap_or(part).trim_end();
    if RESERVED.iter().any(|name| stem.eq_ignore_ascii_case(name))
        || part.ends_with('.')
        || part.ends_with(' ')
        || part
            .chars()
            .any(|c| matches!(c, '<' | '>' | '"' | '|' | '?' | '*'))
    {
        return Err(unsafe_entry(
            component,
            display,
            "name is not a valid Windows file name",
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
#[allow(clippy::unnecessary_wraps)]
fn check_portable_component(
    _component: &str,
    _display: &str,
    _part: &str,
) -> Result<(), PackError> {
    Ok(())
}

fn check_reserved(component: &str, full: &[String]) -> Result<(), PackError> {
    if let [only] = full
        && RESERVED_ROOT_NAMES.contains(&only.as_str())
    {
        return Err(unsafe_entry(
            component,
            only,
            "name is reserved for the install receipt",
        ));
    }
    Ok(())
}

/// A symbolic link target must be relative and resolve, lexically, inside the install root.
fn check_link_target(
    component: &str,
    display: &str,
    full: &[String],
    target: &str,
) -> Result<(), PackError> {
    if target.is_empty() {
        return Err(unsafe_entry(component, display, "empty link target"));
    }
    if target.starts_with('/') {
        return Err(unsafe_entry(component, display, "link target is absolute"));
    }
    if target
        .chars()
        .any(|c| c == '\\' || c == ':' || c.is_control())
    {
        return Err(unsafe_entry(
            component,
            display,
            "link target contains a backslash, ':' or a control character",
        ));
    }
    let mut stack: Vec<&str> = full[..full.len() - 1].iter().map(String::as_str).collect();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if stack.pop().is_none() {
                    return Err(unsafe_entry(
                        component,
                        display,
                        "link target escapes the install root",
                    ));
                }
            }
            part => stack.push(part),
        }
    }
    Ok(())
}

/// Canonicalizes every extracted symbolic link and fails when one is dangling or resolves
/// outside `root`. Runs once all components are extracted, so links may cross components.
pub(crate) fn check_links(root: &Path, links: &[LinkRecord]) -> Result<(), PackError> {
    if links.is_empty() {
        return Ok(());
    }
    let canonical_root = fs::canonicalize(root)
        .map_err(|error| PackError::io("could not resolve the install root", root, error))?;
    for link in links {
        let path = join_parts(root, &link.path);
        match fs::canonicalize(&path) {
            Ok(real) if real.starts_with(&canonical_root) => {}
            Ok(_) => {
                return Err(unsafe_entry(
                    &link.component,
                    &link.path,
                    "symbolic link resolves outside the install root",
                ));
            }
            Err(_) => {
                return Err(unsafe_entry(
                    &link.component,
                    &link.path,
                    "symbolic link is dangling or loops",
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn join_parts(root: &Path, path: &str) -> PathBuf {
    let mut out = root.to_path_buf();
    for part in path.split('/') {
        out.push(part);
    }
    out
}

fn prefixed(dest: &[String], parts: Vec<String>) -> Vec<String> {
    let mut full = dest.to_vec();
    full.extend(parts);
    full
}

fn link_name<R: Read>(
    entry: &tar::Entry<'_, R>,
    component: &str,
    display: &str,
) -> Result<String, PackError> {
    let target = entry
        .link_name_bytes()
        .ok_or_else(|| unsafe_entry(component, display, "link has no target"))?;
    String::from_utf8(target.into_owned())
        .map_err(|_| unsafe_entry(component, display, "link target is not UTF-8"))
}

fn unsafe_entry(component: &str, entry: &str, reason: &'static str) -> PackError {
    PackError::UnsafeArchive {
        component: component.to_owned(),
        entry: entry.to_owned(),
        reason,
    }
}

fn corrupt(component: &str, error: impl std::fmt::Display) -> PackError {
    PackError::CorruptArchive {
        component: component.to_owned(),
        detail: error.to_string(),
    }
}

fn tar_error(component: &str, budget: &Budget<'_>, error: io::Error) -> PackError {
    if error.kind() == io::ErrorKind::FileTooLarge {
        PackError::ExtractLimit {
            component: component.to_owned(),
            what: "unpacked size",
            limit: budget.bounds.max_bytes,
        }
    } else {
        corrupt(component, error)
    }
}

/// Fails with `FileTooLarge` once the decompressed stream exceeds its budget, which also
/// bounds what the tar reader buffers for long names and PAX records.
struct LimitedReader<R> {
    inner: R,
    remaining: u64,
}

impl<R: Read> Read for LimitedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            let mut probe = [0_u8; 1];
            return match self.inner.read(&mut probe)? {
                0 => Ok(0),
                _ => Err(io::Error::new(
                    io::ErrorKind::FileTooLarge,
                    "decompressed stream exceeds the declared size",
                )),
            };
        }
        let len = buf
            .len()
            .min(usize::try_from(self.remaining).unwrap_or(usize::MAX));
        let read = self.inner.read(&mut buf[..len])?;
        self.remaining -= read as u64;
        Ok(read)
    }
}

struct Writer<'a> {
    root: &'a Path,
    component: &'a str,
    files: Vec<FileRecord>,
    file_index: HashMap<String, usize>,
    links: Vec<LinkRecord>,
    cancel: &'a CancellationToken,
}

impl Writer<'_> {
    /// Creates missing parent directories and refuses to pass through anything that is not a
    /// real directory, so no entry is written through a symbolic link.
    fn prepare_parent(&self, full: &[String]) -> Result<PathBuf, PackError> {
        let mut path = self.root.to_path_buf();
        for part in &full[..full.len() - 1] {
            path.push(part);
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => {
                    return Err(unsafe_entry(
                        self.component,
                        &full.join("/"),
                        "path passes through a file or symbolic link",
                    ));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    fs::create_dir(&path)
                        .map_err(|error| PackError::io("could not create", &path, error))?;
                }
                Err(error) => return Err(PackError::io("could not inspect", &path, error)),
            }
        }
        path.push(&full[full.len() - 1]);
        Ok(path)
    }

    fn create_file(
        &self,
        path: &Path,
        full: &[String],
        executable: bool,
    ) -> Result<File, PackError> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(if executable { 0o755 } else { 0o644 });
        }
        #[cfg(not(unix))]
        let _ = executable;
        options.open(path).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                unsafe_entry(self.component, &full.join("/"), "duplicate entry")
            } else {
                PackError::io("could not create", path, error)
            }
        })
    }

    fn copy_into(
        &self,
        reader: &mut dyn Read,
        file: &mut File,
        path: &Path,
        budget: &mut Budget<'_>,
    ) -> Result<(String, u64), PackError> {
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
        let mut size = 0_u64;
        loop {
            if self.cancel.is_cancelled() {
                return Err(PackError::Interrupted);
            }
            let read = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::FileTooLarge => {
                    return Err(PackError::ExtractLimit {
                        component: self.component.to_owned(),
                        what: "unpacked size",
                        limit: budget.bounds.max_bytes,
                    });
                }
                Err(error) => return Err(corrupt(self.component, error)),
            };
            budget.count_bytes(read as u64)?;
            hasher.update(&buffer[..read]);
            file.write_all(&buffer[..read])
                .map_err(|error| PackError::io("could not write", path, error))?;
            size += read as u64;
        }
        Ok((hex::encode(hasher.finalize()), size))
    }

    fn record_file(&mut self, full: &[String], sha256: String, size: u64) {
        let path = full.join("/");
        self.file_index.insert(path.clone(), self.files.len());
        self.files.push(FileRecord { path, sha256, size });
    }
}

impl Sink for Writer<'_> {
    fn dir(&mut self, full: &[String]) -> Result<(), PackError> {
        let path = self.prepare_parent(full)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => Ok(()),
            Ok(_) => Err(unsafe_entry(
                self.component,
                &full.join("/"),
                "directory replaces a file or symbolic link",
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(&path)
                .map_err(|error| PackError::io("could not create", &path, error)),
            Err(error) => Err(PackError::io("could not inspect", &path, error)),
        }
    }

    fn file(
        &mut self,
        full: &[String],
        reader: &mut dyn Read,
        executable: bool,
        budget: &mut Budget<'_>,
    ) -> Result<(), PackError> {
        let path = self.prepare_parent(full)?;
        let mut file = self.create_file(&path, full, executable)?;
        let (sha256, size) = self.copy_into(reader, &mut file, &path, budget)?;
        self.record_file(full, sha256, size);
        Ok(())
    }

    #[cfg(unix)]
    fn symlink(&mut self, full: &[String], target: &str) -> Result<(), PackError> {
        let path = self.prepare_parent(full)?;
        std::os::unix::fs::symlink(target, &path).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                unsafe_entry(self.component, &full.join("/"), "duplicate entry")
            } else {
                PackError::io("could not create the link", &path, error)
            }
        })?;
        self.links.push(LinkRecord {
            component: self.component.to_owned(),
            path: full.join("/"),
            target: target.to_owned(),
        });
        Ok(())
    }

    #[cfg(not(unix))]
    fn symlink(&mut self, full: &[String], _target: &str) -> Result<(), PackError> {
        Err(unsafe_entry(
            self.component,
            &full.join("/"),
            "symbolic links are not supported in packs on this platform",
        ))
    }

    fn hardlink(
        &mut self,
        full: &[String],
        target: &[String],
        budget: &mut Budget<'_>,
    ) -> Result<(), PackError> {
        let key = target.join("/");
        let Some(&index) = self.file_index.get(&key) else {
            return Err(unsafe_entry(
                self.component,
                &full.join("/"),
                "hard link target is not an earlier regular file of the component",
            ));
        };
        let source_path = join_parts(self.root, &key);
        let mut source = File::open(&source_path)
            .map_err(|error| PackError::io("could not open", &source_path, error))?;
        let executable = executable_bits(&source);
        let path = self.prepare_parent(full)?;
        let mut file = self.create_file(&path, full, executable)?;
        let (sha256, size) = self.copy_into(&mut source, &mut file, &path, budget)?;
        debug_assert_eq!(sha256, self.files[index].sha256);
        self.record_file(full, sha256, size);
        Ok(())
    }
}

#[cfg(unix)]
fn executable_bits(file: &File) -> bool {
    use std::os::unix::fs::PermissionsExt;
    file.metadata()
        .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable_bits(_file: &File) -> bool {
    false
}

/// Validates an archive without writing it.
#[derive(Default)]
struct Scanner {
    component: String,
    files: HashMap<String, u64>,
    dirs: HashSet<String>,
    links: HashSet<String>,
}

impl Scanner {
    fn check_new(&self, full: &[String], allow_dir: bool) -> Result<(), PackError> {
        for end in 1..full.len() {
            let ancestor = full[..end].join("/");
            if self.files.contains_key(&ancestor) || self.links.contains(&ancestor) {
                return Err(unsafe_entry(
                    &self.component,
                    &full.join("/"),
                    "path passes through a file or symbolic link",
                ));
            }
        }
        let path = full.join("/");
        if self.files.contains_key(&path)
            || self.links.contains(&path)
            || (!allow_dir && self.dirs.contains(&path))
        {
            return Err(unsafe_entry(&self.component, &path, "duplicate entry"));
        }
        Ok(())
    }
}

impl Sink for Scanner {
    fn dir(&mut self, full: &[String]) -> Result<(), PackError> {
        self.check_new(full, true)?;
        self.dirs.insert(full.join("/"));
        Ok(())
    }

    fn file(
        &mut self,
        full: &[String],
        reader: &mut dyn Read,
        _executable: bool,
        budget: &mut Budget<'_>,
    ) -> Result<(), PackError> {
        self.check_new(full, false)?;
        let size =
            io::copy(reader, &mut io::sink()).map_err(|error| corrupt(&self.component, error))?;
        budget.count_bytes(size)?;
        self.files.insert(full.join("/"), size);
        Ok(())
    }

    fn symlink(&mut self, full: &[String], _target: &str) -> Result<(), PackError> {
        self.check_new(full, false)?;
        self.links.insert(full.join("/"));
        Ok(())
    }

    fn hardlink(
        &mut self,
        full: &[String],
        target: &[String],
        budget: &mut Budget<'_>,
    ) -> Result<(), PackError> {
        self.check_new(full, false)?;
        let Some(&size) = self.files.get(&target.join("/")) else {
            return Err(unsafe_entry(
                &self.component,
                &full.join("/"),
                "hard link target is not an earlier regular file of the component",
            ));
        };
        budget.count_bytes(size)?;
        self.files.insert(full.join("/"), size);
        Ok(())
    }
}

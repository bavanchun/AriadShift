//! Path confinement and validation for MCP file operations.
//!
//! Confines all input reads and output writes to client roots and explicit `--allow-dir`
//! directories. Confinement checks are backed by capability handles (`cap_std::fs::Dir`)
//! opened once per root, eliminating TOCTOU directory swap attacks. Confinement checks are
//! independent of the MCP transport so they can be tested in isolation and reused across commands.

use std::{
    ffi::OsString,
    fs, io,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

#[cfg(unix)]
use cap_std::fs::OpenOptionsExt;

use crate::convert::DocumentFormat;

/// Errors arising from path confinement violations.
#[derive(Debug, thiserror::Error)]
pub enum ConfineError {
    #[error("allowed directory set is empty; pass --allow-dir or provide client roots")]
    EmptyAllowedSet,

    #[error("path is outside allowed directories: {path}")]
    AccessDenied { path: String },

    #[error("input document not found: {path}")]
    InputNotFound { path: String },

    #[error("input is not a regular file: {path}")]
    NotRegularFile { path: String },

    #[error("output directory not found: {path}")]
    OutputParentNotFound { path: String },

    #[error("hidden path components are not allowed: {path}")]
    HiddenComponent { path: String },

    #[error("output extension does not match target format '{expected}': {path}")]
    ExtensionMismatch { path: String, expected: String },

    #[error("destination already exists: {path}")]
    DestinationExists { path: String },

    #[error("existing symlink destination is refused: {path}")]
    SymlinkDestinationRefused { path: String },

    #[error("output destination cannot be the same as input file")]
    SameFileAsInput,

    #[error("invalid URI or path: {message}")]
    InvalidPath { message: String },

    #[error("I/O error during confinement check: {0}")]
    Io(#[from] io::Error),
}

impl ConfineError {
    /// Maps confinement error to CLI exit code.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::DestinationExists { .. } | Self::SymlinkDestinationRefused { .. } => 6,
            Self::EmptyAllowedSet
            | Self::AccessDenied { .. }
            | Self::HiddenComponent { .. }
            | Self::ExtensionMismatch { .. }
            | Self::SameFileAsInput
            | Self::OutputParentNotFound { .. }
            | Self::InvalidPath { .. } => 2,
            Self::InputNotFound { .. } | Self::NotRegularFile { .. } | Self::Io(_) => 1,
        }
    }

    /// Short machine-readable error code string.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::EmptyAllowedSet => "empty_allowed_set",
            Self::AccessDenied { .. } => "access_denied",
            Self::InputNotFound { .. } => "input_not_found",
            Self::NotRegularFile { .. } => "not_regular_file",
            Self::OutputParentNotFound { .. } => "output_parent_not_found",
            Self::HiddenComponent { .. } => "hidden_component",
            Self::ExtensionMismatch { .. } => "extension_mismatch",
            Self::DestinationExists { .. } => "destination_exists",
            Self::SymlinkDestinationRefused { .. } => "symlink_destination_refused",
            Self::SameFileAsInput => "destination_same_as_input",
            Self::InvalidPath { .. } => "invalid_path",
            Self::Io(_) => "io_error",
        }
    }
}

/// A single allowed root directory backed by an open capability handle.
#[derive(Clone)]
pub struct AllowedRoot {
    pub canonical_path: PathBuf,
    pub dir: Arc<cap_std::fs::Dir>,
}

impl std::fmt::Debug for AllowedRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AllowedRoot")
            .field("canonical_path", &self.canonical_path)
            .finish_non_exhaustive()
    }
}

/// The set of canonicalized directories permitted for reading and writing,
/// each backed by an open capability handle to enforce path confinement without ambient authority.
#[derive(Clone, Debug, Default)]
pub struct AllowedDirs {
    roots: Vec<AllowedRoot>,
}

impl PartialEq for AllowedDirs {
    fn eq(&self, other: &Self) -> bool {
        self.dirs() == other.dirs()
    }
}

impl Eq for AllowedDirs {}

impl AllowedDirs {
    /// Creates an `AllowedDirs` set by canonicalizing `--allow-dir` paths and `file://` client roots
    /// and opening each as a `cap_std::fs::Dir` capability handle once.
    ///
    /// Non-existent `--allow-dir` directories produce an error. Non-existent roots are skipped.
    pub fn new(allow_dirs: &[PathBuf], roots: &[String]) -> Result<Self, ConfineError> {
        let mut root_entries = Vec::new();

        for dir in allow_dirs {
            validate_path_string(dir)?;
            let canonical = fs::canonicalize(dir).map_err(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    ConfineError::AccessDenied {
                        path: dir.display().to_string(),
                    }
                } else {
                    ConfineError::Io(e)
                }
            })?;
            if !canonical.is_dir() {
                return Err(ConfineError::AccessDenied {
                    path: dir.display().to_string(),
                });
            }
            if !root_entries
                .iter()
                .any(|r: &AllowedRoot| r.canonical_path == canonical)
            {
                let cap_dir =
                    cap_std::fs::Dir::open_ambient_dir(&canonical, cap_std::ambient_authority())
                        .map_err(ConfineError::Io)?;
                root_entries.push(AllowedRoot {
                    canonical_path: canonical,
                    dir: Arc::new(cap_dir),
                });
            }
        }

        for root in roots {
            let Ok(path) = parse_file_uri(root) else {
                continue;
            };
            if let Ok(canonical) = fs::canonicalize(&path) {
                let is_new_dir = canonical.is_dir()
                    && !root_entries
                        .iter()
                        .any(|r: &AllowedRoot| r.canonical_path == canonical);
                if is_new_dir
                    && let Ok(cap_dir) =
                        cap_std::fs::Dir::open_ambient_dir(&canonical, cap_std::ambient_authority())
                {
                    root_entries.push(AllowedRoot {
                        canonical_path: canonical,
                        dir: Arc::new(cap_dir),
                    });
                }
            }
        }

        Ok(Self {
            roots: root_entries,
        })
    }

    /// Returns true if no allowed directories are configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// Returns the slice of allowed root entries.
    #[must_use]
    pub fn roots(&self) -> &[AllowedRoot] {
        &self.roots
    }

    /// Returns the vector of canonicalized allowed directory paths.
    #[must_use]
    pub fn dirs(&self) -> Vec<PathBuf> {
        self.roots
            .iter()
            .map(|r| r.canonical_path.clone())
            .collect()
    }
}

/// A validated input document with an open `File` handle resolved through capability routing.
#[derive(Debug)]
pub struct ConfinedInput {
    /// Canonical filesystem path.
    pub path: PathBuf,
    /// Path relative to the containing allowed root.
    pub relative_path: PathBuf,
    /// An open File handle to the input file, opened through the capability handle with nofollow.
    pub file: fs::File,
}

impl Clone for ConfinedInput {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            relative_path: self.relative_path.clone(),
            file: self.file.try_clone().expect("clone file handle"),
        }
    }
}

impl ConfinedInput {
    /// Copies the opened file into a destination file, bounded by `max_bytes`.
    pub fn copy_to_file(&mut self, dest: &mut fs::File, max_bytes: Option<u64>) -> io::Result<u64> {
        use std::io::{Read, Seek, SeekFrom, Write};
        self.file.seek(SeekFrom::Start(0))?;
        let mut buffer = [0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            let n = self.file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            total = total.saturating_add(n as u64);
            if let Some(max) = max_bytes
                && total > max
            {
                return Err(io::Error::new(
                    io::ErrorKind::FileTooLarge,
                    "input file exceeds byte limit",
                ));
            }
            dest.write_all(&buffer[..n])?;
        }
        dest.flush()?;
        Ok(total)
    }

    /// Creates an isolated temporary directory containing a copy of the validated input.
    ///
    /// This guarantees that any subsequent operations (such as inspection, planning, or conversion)
    /// read only the verified bytes and cannot be affected by concurrent symlink swaps or path races.
    pub fn create_isolated_copy(
        &mut self,
        file_name: Option<&str>,
        max_bytes: Option<u64>,
    ) -> io::Result<(tempfile::TempDir, PathBuf)> {
        let temp_dir = tempfile::tempdir()?;
        let stem = file_name
            .or_else(|| self.path.file_name().and_then(|f| f.to_str()))
            .unwrap_or("input.bin");
        let dest_path = temp_dir.path().join(stem);
        let mut dest_file = fs::File::create(&dest_path)?;
        self.copy_to_file(&mut dest_file, max_bytes)?;
        Ok((temp_dir, dest_path))
    }
}

impl std::ops::Deref for ConfinedInput {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

impl AsRef<Path> for ConfinedInput {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

/// Validates an input file path against the allowed directory set and opens a handle to it.
///
/// Ensures the file exists, is a regular file, and lies strictly within an allowed root.
/// The containment check runs on the canonical path BEFORE any stat of the target, and
/// every failure on a path outside the set returns `AccessDenied` to prevent existence or type oracles.
/// The returned `ConfinedInput` holds the open regular file handle.
pub fn confine_input(
    input_path: &Path,
    allowed: &AllowedDirs,
) -> Result<ConfinedInput, ConfineError> {
    if allowed.is_empty() {
        return Err(ConfineError::EmptyAllowedSet);
    }

    validate_path_string(input_path)?;

    if !input_path.is_absolute() {
        return Err(ConfineError::InvalidPath {
            message: format!(
                "relative path '{}' is not permitted; provide an absolute path or file:// URI",
                input_path.display()
            ),
        });
    }

    let display_str = input_path.display().to_string();

    let canonical = match fs::canonicalize(input_path) {
        Ok(c) => c,
        Err(e) => {
            // Map every failure on an ancestor outside allowed directories to AccessDenied (no oracle)
            let mut cur = input_path.parent();
            let mut inside = false;
            while let Some(parent) = cur {
                if let Ok(canon_parent) = fs::canonicalize(parent) {
                    if is_inside_allowed(&canon_parent, allowed) {
                        inside = true;
                    }
                    break;
                }
                cur = parent.parent();
            }
            if !inside {
                return Err(ConfineError::AccessDenied { path: display_str });
            }
            if e.kind() == io::ErrorKind::NotFound {
                return Err(ConfineError::InputNotFound { path: display_str });
            }
            return Err(ConfineError::Io(e));
        }
    };

    // Containment check runs before any metadata inspection
    let matching_root = allowed
        .roots()
        .iter()
        .find(|root| is_subpath(&canonical, &root.canonical_path));

    let Some(root) = matching_root else {
        return Err(ConfineError::AccessDenied { path: display_str });
    };

    let relative = canonical
        .strip_prefix(&root.canonical_path)
        .map_err(|_| ConfineError::AccessDenied {
            path: display_str.clone(),
        })?
        .to_path_buf();

    // Check hidden components on relative path
    for comp in relative.components() {
        match comp {
            Component::Normal(c) if c.to_string_lossy().starts_with('.') => {
                return Err(ConfineError::HiddenComponent { path: display_str });
            }
            Component::Normal(_) => {}
            _ => {
                return Err(ConfineError::AccessDenied { path: display_str });
            }
        }
    }

    // Resolve and check via capability handle inside the sandbox
    let sym_meta = match root.dir.symlink_metadata(&relative) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(ConfineError::InputNotFound { path: display_str });
        }
        Err(e) => return Err(ConfineError::Io(e)),
    };

    if sym_meta.is_symlink() {
        return Err(ConfineError::AccessDenied { path: display_str });
    }
    if !sym_meta.is_file() {
        return Err(ConfineError::NotRegularFile { path: display_str });
    }

    let file = open_file_nofollow(&root.dir, &relative).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            ConfineError::InputNotFound {
                path: display_str.clone(),
            }
        } else {
            ConfineError::Io(e)
        }
    })?;

    let post_meta = file.metadata()?;
    if !post_meta.is_file() {
        return Err(ConfineError::NotRegularFile { path: display_str });
    }

    Ok(ConfinedInput {
        path: canonical,
        relative_path: relative.to_path_buf(),
        file,
    })
}

pub fn open_file_nofollow(root_dir: &cap_std::fs::Dir, relative: &Path) -> io::Result<fs::File> {
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let file_name = relative
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing file name"))?;

    let parent_dir = if parent.as_os_str().is_empty() {
        root_dir.try_clone()?
    } else {
        root_dir.open_dir(parent)?
    };

    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let cap_file = parent_dir.open_with(file_name, &options)?;
    Ok(cap_file.into_std())
}

/// Validated output target with capability handle to the parent directory.
#[derive(Debug)]
pub struct ValidatedOutput {
    /// Full path to the destination output file.
    pub destination: PathBuf,
    /// Final filename inside the parent directory.
    pub file_name: OsString,
    /// Open capability handle to the canonical parent directory.
    pub parent_dir: cap_std::fs::Dir,
    /// Whether an existing destination file may be replaced.
    pub overwrite: bool,
}

/// Validates an output destination path, opens a capability handle to its parent directory
/// through the allowed root handle, and enforces extension, hidden component, overwrite,
/// and confinement rules.
pub fn confine_output(
    input_path: &Path,
    output_path: Option<&Path>,
    target_format: &str,
    overwrite_flag: bool,
    allow_overwrite: bool,
    allowed: &AllowedDirs,
) -> Result<ValidatedOutput, ConfineError> {
    if allowed.is_empty() {
        return Err(ConfineError::EmptyAllowedSet);
    }

    let expected_fmt =
        DocumentFormat::parse(target_format).ok_or_else(|| ConfineError::InvalidPath {
            message: format!("unrecognized target format: {target_format}"),
        })?;

    let output = match output_path {
        Some(path) => {
            validate_path_string(path)?;
            if !path.is_absolute() {
                return Err(ConfineError::InvalidPath {
                    message: format!(
                        "relative path '{}' is not permitted; provide an absolute path or file:// URI",
                        path.display()
                    ),
                });
            }
            path.to_path_buf()
        }
        None => input_path.with_extension(expected_fmt.default_extension()),
    };

    let display_str = output.display().to_string();

    // Check extension matches target format
    let ext = output.extension().and_then(|e| e.to_str()).unwrap_or("");
    let actual_fmt = DocumentFormat::from_extension(ext);
    if actual_fmt != Some(expected_fmt) {
        return Err(ConfineError::ExtensionMismatch {
            path: display_str.clone(),
            expected: expected_fmt.as_str().to_string(),
        });
    }

    let file_name = output
        .file_name()
        .ok_or_else(|| ConfineError::InvalidPath {
            message: "destination path has no filename".to_owned(),
        })?
        .to_os_string();

    // Destination filename must not be a dotfile
    if file_name.to_string_lossy().starts_with('.') {
        return Err(ConfineError::HiddenComponent {
            path: display_str.clone(),
        });
    }

    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("/"));

    // Check hidden directory components relative to allowed roots on uncanonicalized parent
    for root in allowed.roots() {
        if let Ok(rel) = parent.strip_prefix(&root.canonical_path) {
            for comp in rel.components() {
                if let Component::Normal(c) = comp
                    && c.to_string_lossy().starts_with('.')
                {
                    return Err(ConfineError::HiddenComponent {
                        path: display_str.clone(),
                    });
                }
            }
        }
    }

    let canonical_parent = match fs::canonicalize(parent) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // Check if parent was within an allowed root to report OutputParentNotFound vs AccessDenied
            let is_inside = allowed
                .roots()
                .iter()
                .any(|r| is_subpath(parent, &r.canonical_path));
            if is_inside {
                return Err(ConfineError::OutputParentNotFound {
                    path: parent.display().to_string(),
                });
            }
            let mut cur = parent.parent();
            let mut inside_ancestor = false;
            while let Some(p) = cur {
                if let Ok(canon_p) = fs::canonicalize(p) {
                    if is_inside_allowed(&canon_p, allowed) {
                        inside_ancestor = true;
                    }
                    break;
                }
                cur = p.parent();
            }
            if inside_ancestor {
                return Err(ConfineError::OutputParentNotFound {
                    path: parent.display().to_string(),
                });
            }
            return Err(ConfineError::AccessDenied { path: display_str });
        }
        Err(e) => return Err(ConfineError::Io(e)),
    };

    let matching_root = allowed
        .roots()
        .iter()
        .find(|root| is_subpath(&canonical_parent, &root.canonical_path));

    let Some(root) = matching_root else {
        return Err(ConfineError::AccessDenied { path: display_str });
    };

    let rel_parent = canonical_parent
        .strip_prefix(&root.canonical_path)
        .map_err(|_| ConfineError::AccessDenied {
            path: display_str.clone(),
        })?;

    // Check hidden directory components within allowed roots
    for comp in rel_parent.components() {
        if let Component::Normal(c) = comp
            && c.to_string_lossy().starts_with('.')
        {
            return Err(ConfineError::HiddenComponent {
                path: display_str.clone(),
            });
        }
    }

    // Open capability handle to parent directory THROUGH the root handle (no ambient authority)
    let parent_dir = if rel_parent.as_os_str().is_empty() {
        (*root.dir).try_clone().map_err(ConfineError::Io)?
    } else {
        root.dir.open_dir(rel_parent).map_err(ConfineError::Io)?
    };

    // Check existing destination via capability handle
    match parent_dir.symlink_metadata(&file_name) {
        Ok(meta) => {
            if meta.is_symlink() {
                return Err(ConfineError::SymlinkDestinationRefused {
                    path: display_str.clone(),
                });
            }
            if !(allow_overwrite && overwrite_flag) {
                return Err(ConfineError::DestinationExists {
                    path: display_str.clone(),
                });
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(ConfineError::Io(e)),
    }

    let destination = canonical_parent.join(&file_name);

    // Refuse output that canonicalizes to the input path
    let is_same_file = match fs::canonicalize(input_path) {
        Ok(inp) => destination == inp,
        _ => false,
    };
    if is_same_file {
        return Err(ConfineError::SameFileAsInput);
    }

    Ok(ValidatedOutput {
        destination,
        file_name,
        parent_dir,
        overwrite: overwrite_flag && allow_overwrite,
    })
}

/// Checks whether a canonicalized target path is inside any of the allowed directories.
#[must_use]
pub fn is_inside_allowed(canonical_path: &Path, allowed: &AllowedDirs) -> bool {
    allowed
        .roots()
        .iter()
        .any(|root| is_subpath(canonical_path, &root.canonical_path))
}

/// Returns true if `target` is equal to `base` or is a descendant of `base` component-wise.
#[must_use]
pub fn is_subpath(target: &Path, base: &Path) -> bool {
    let mut target_comps = target.components();

    let mut base_comps = base.components();

    loop {
        match (base_comps.next(), target_comps.next()) {
            (Some(b), Some(t)) => {
                if !components_equal(b, t) {
                    return false;
                }
            }
            (None, remaining) => {
                // All base components matched. If target has remaining components, verify no traversal.
                if let Some(r) = remaining {
                    if !matches!(r, Component::Normal(_)) {
                        return false;
                    }
                    for comp in target_comps {
                        if !matches!(comp, Component::Normal(_)) {
                            return false;
                        }
                    }
                }
                return true;
            }
            (Some(_), None) => {
                return false;
            }
        }
    }
}

fn components_equal(a: Component, b: Component) -> bool {
    #[cfg(windows)]
    {
        match (a, b) {
            (Component::Prefix(pa), Component::Prefix(pb)) => pa
                .as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(&pb.as_os_str().to_string_lossy()),
            (Component::RootDir, Component::RootDir) => true,
            (Component::Normal(na), Component::Normal(nb)) => na
                .to_string_lossy()
                .eq_ignore_ascii_case(&nb.to_string_lossy()),
            _ => a == b,
        }
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}

/// Parses a `file://` URI into a local filesystem path with percent-decoding and authority validation.
///
/// Rejects non-local authorities (only empty or `localhost` permitted) and relative paths.
pub fn parse_file_uri(uri: &str) -> Result<PathBuf, ConfineError> {
    let rest = uri
        .strip_prefix("file://")
        .ok_or_else(|| ConfineError::InvalidPath {
            message: format!("URI does not use file:// scheme: {uri}"),
        })?;

    let path_part = if let Some(stripped) = rest.strip_prefix("localhost/") {
        format!("/{stripped}")
    } else if rest.starts_with('/') {
        rest.to_owned()
    } else {
        // Non-empty authority or malformed URI
        return Err(ConfineError::InvalidPath {
            message: format!("non-local file:// URI authority not allowed: {uri}"),
        });
    };

    let decoded = percent_decode(&path_part)?;

    #[cfg(windows)]
    {
        let s = decoded.strip_prefix('/').unwrap_or(&decoded);
        let cleaned = strip_verbatim_prefix(Path::new(s));
        let path = PathBuf::from(cleaned.to_string_lossy().replace('/', "\\"));
        if !path.is_absolute() {
            return Err(ConfineError::InvalidPath {
                message: "URI path must be an absolute path".to_owned(),
            });
        }
        validate_path_string(&path)?;
        Ok(path)
    }
    #[cfg(not(windows))]
    {
        let path = PathBuf::from(decoded);
        if !path.is_absolute() {
            return Err(ConfineError::InvalidPath {
                message: "URI path must be an absolute path".to_owned(),
            });
        }
        validate_path_string(&path)?;
        Ok(path)
    }
}

fn percent_decode(s: &str) -> Result<String, ConfineError> {
    let mut bytes = Vec::with_capacity(s.len());
    let mut chars = s.bytes();

    while let Some(b) = chars.next() {
        if b == b'%' {
            let h1 = chars.next().ok_or_else(|| ConfineError::InvalidPath {
                message: "incomplete percent escape sequence in URI".to_owned(),
            })?;
            let h2 = chars.next().ok_or_else(|| ConfineError::InvalidPath {
                message: "incomplete percent escape sequence in URI".to_owned(),
            })?;
            let val = decode_hex_byte(h1, h2).ok_or_else(|| ConfineError::InvalidPath {
                message: "invalid hex digits in percent escape sequence".to_owned(),
            })?;
            if val == 0 {
                return Err(ConfineError::InvalidPath {
                    message: "NUL byte in URI path".to_owned(),
                });
            }
            bytes.push(val);
        } else {
            if b == 0 {
                return Err(ConfineError::InvalidPath {
                    message: "NUL byte in URI path".to_owned(),
                });
            }
            bytes.push(b);
        }
    }

    String::from_utf8(bytes).map_err(|_| ConfineError::InvalidPath {
        message: "URI path is not valid UTF-8".to_owned(),
    })
}

fn decode_hex_byte(h1: u8, h2: u8) -> Option<u8> {
    let v1 = hex_digit_val(h1)?;
    let v2 = hex_digit_val(h2)?;
    Some((v1 << 4) | v2)
}

fn hex_digit_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Validates path string for NUL bytes, Windows device stems, and Windows ADS colons.
pub fn validate_path_string(path: &Path) -> Result<(), ConfineError> {
    let s = path.as_os_str().to_string_lossy();
    if s.contains('\0') {
        return Err(ConfineError::InvalidPath {
            message: "path contains NUL byte".to_owned(),
        });
    }

    #[cfg(windows)]
    {
        validate_windows_path_syntax(&s)?;
    }

    Ok(())
}

/// Pure string validator for Windows path syntax (ADS ':' and reserved device stems).
pub fn validate_windows_path_syntax(path_str: &str) -> Result<(), ConfineError> {
    if path_str.contains('\0') {
        return Err(ConfineError::InvalidPath {
            message: "path contains NUL byte".to_owned(),
        });
    }

    let cleaned = strip_verbatim_prefix_str(path_str);
    let parts: Vec<&str> = cleaned.split(['/', '\\']).collect();

    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }

        // Drive specifier like "C:" is permitted at the beginning
        if i == 0
            && part.len() == 2
            && part.as_bytes()[1] == b':'
            && part.as_bytes()[0].is_ascii_alphabetic()
        {
            continue;
        }

        if part.contains(':') {
            return Err(ConfineError::InvalidPath {
                message: format!("path references Windows alternate data stream: {part}"),
            });
        }

        let stem = part.split('.').next().unwrap_or(part);
        if is_windows_device_stem(stem) {
            return Err(ConfineError::InvalidPath {
                message: format!("path references reserved Windows device name: {part}"),
            });
        }
    }

    Ok(())
}

/// Checks if a component stem matches a reserved Windows device name without byte slicing panics.
#[must_use]
pub fn is_windows_device_stem(stem: &str) -> bool {
    let s = stem.trim();
    if s.eq_ignore_ascii_case("CON")
        || s.eq_ignore_ascii_case("PRN")
        || s.eq_ignore_ascii_case("AUX")
        || s.eq_ignore_ascii_case("NUL")
    {
        return true;
    }

    let chars: Vec<char> = s.chars().collect();
    if chars.len() == 4 {
        let prefix: String = chars[..3].iter().collect();
        let last = chars[3];
        if (prefix.eq_ignore_ascii_case("COM") || prefix.eq_ignore_ascii_case("LPT"))
            && ('1'..='9').contains(&last)
        {
            return true;
        }
    }

    false
}

/// Strips Windows verbatim prefix (`\\?\` or `\\?\UNC\`) from a path.
#[must_use]
pub fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let s = path.as_os_str().to_string_lossy();
    PathBuf::from(strip_verbatim_prefix_str(&s))
}

fn strip_verbatim_prefix_str(s: &str) -> &str {
    if let Some(stripped) = s.strip_prefix(r"\\?\UNC\") {
        stripped
    } else if let Some(stripped) = s.strip_prefix(r"\\?\") {
        stripped
    } else {
        s
    }
}

/// Converts a filesystem path into a valid `file://` URI string with percent-encoded components.
#[must_use]
pub fn path_to_file_uri(path: &Path) -> String {
    let cleaned = strip_verbatim_prefix(path);
    let p = cleaned.to_string_lossy();
    #[cfg(windows)]
    {
        windows_path_to_file_uri(&p)
    }
    #[cfg(not(windows))]
    {
        let encoded = encode_uri_path(&p);
        if encoded.starts_with('/') {
            format!("file://{encoded}")
        } else {
            format!("file:///{encoded}")
        }
    }
}

/// Pure string conversion of Windows path to `file://` URI with percent-encoding.
#[must_use]
pub fn windows_path_to_file_uri(path_str: &str) -> String {
    let cleaned = strip_verbatim_prefix_str(path_str);
    let normalized = cleaned.replace('\\', "/");
    let is_unc = cleaned.starts_with(r"\\") || cleaned.starts_with("//");

    if is_unc {
        let trimmed = normalized.trim_start_matches('/');
        let encoded = encode_uri_path(trimmed);
        format!("file://{encoded}")
    } else {
        let trimmed = normalized.trim_start_matches('/');
        let encoded = encode_uri_path(trimmed);
        format!("file:///{encoded}")
    }
}

fn encode_uri_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                out.push(b as char);
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{:02X}", b);
            }
        }
    }
    out
}

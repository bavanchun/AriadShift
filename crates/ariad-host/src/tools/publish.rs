//! Atomic multi-artifact staging and publication (`docs/tools.md` §6, §7).
//!
//! All tool outputs are written into `<workspace>/out/` first. [`publish_atomic`] validates every
//! staged artifact (regular file inside `<workspace>/out/`, no symlinks, no `..` traversal, confined
//! to [`AllowedDirs`] when set, never overwriting any input file) and promotes the batch into the
//! destination directory through a `cap_std::fs::Dir` capability handle. If promoting artifact `k` of
//! `N` fails for any reason, artifacts `0..k` created during the batch are unlinked before returning
//! the error so partial outputs never remain on disk.

use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
};

use ariad_core::{
    format::Format,
    tools::{NamingTemplateError, ToolSpec, normalize_tool_format, validate_naming_template},
};
use cap_std::{ambient_authority, fs::Dir};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    confine::AllowedDirs,
    convert::paths_refer_to_same_file,
    tools::run::ToolError,
    workspace::{Workspace, WorkspaceError},
};

/// Validates that an explicit output file path's extension matches `spec_output_format`
/// using the same extension rules as the confined conversion path.
pub fn validate_output_path_extension(
    output_path: &Path,
    spec_output_format: &str,
) -> Result<(), ToolError> {
    let expected_fmt = normalize_tool_format(spec_output_format).ok_or_else(|| {
        ToolError::EngineFailure(format!(
            "unrecognized tool output format '{spec_output_format}'"
        ))
    })?;
    let ext = output_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    if normalize_tool_format(ext) != Some(expected_fmt) {
        return Err(ToolError::Confinement(format!(
            "output path '{}' extension does not match target format '{expected_fmt}'",
            output_path.display(),
        )));
    }
    Ok(())
}

/// Validates that an explicit output file path's extension is allowed for `spec` and `params`.
pub fn validate_output_path_for_spec(
    output_path: &Path,
    spec: &ToolSpec,
    params: &Value,
    emits_json: bool,
) -> Result<(), ToolError> {
    if emits_json {
        return validate_output_path_extension(output_path, "json");
    }
    let ext = output_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let Some(ext_fmt) = normalize_tool_format(ext) else {
        return Err(ToolError::Confinement(format!(
            "output path '{}' extension does not match target format '{}'",
            output_path.display(),
            spec.output_format
        )));
    };
    if !spec.allows_artifact_format(ext_fmt, params) {
        return Err(ToolError::Confinement(format!(
            "output path '{}' extension '{}' is not allowed for tool '{}'",
            output_path.display(),
            ext,
            spec.id
        )));
    }
    Ok(())
}

/// Validates that every staged artifact's reported format, staged filename extension, and target
/// filename extension match `spec_output_format`.
pub fn validate_artifact_extensions(
    spec_output_format: &str,
    staged: &[StagedArtifact],
) -> Result<(), ToolError> {
    let expected_fmt = normalize_tool_format(spec_output_format).ok_or_else(|| {
        ToolError::EngineFailure(format!(
            "unrecognized tool output format '{spec_output_format}'"
        ))
    })?;

    for item in staged {
        let reported_fmt = normalize_tool_format(&item.format);
        if reported_fmt != Some(expected_fmt) {
            return Err(ToolError::EngineFailure(format!(
                "artifact '{}' reported format '{}', expected '{expected_fmt}'",
                item.staged_path.display(),
                item.format,
            )));
        }

        let staged_ext = item
            .staged_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if normalize_tool_format(staged_ext) != Some(expected_fmt) {
            return Err(ToolError::EngineFailure(format!(
                "staged artifact '{}' extension does not match expected format '{expected_fmt}'",
                item.staged_path.display(),
            )));
        }

        validate_output_path_extension(Path::new(&item.target_name), expected_fmt)?;
    }

    Ok(())
}

/// Validates that every staged artifact's reported format, staged filename extension, and target
/// filename extension are allowed by `spec` (`output_format` / `output_formats`) and `params`.
pub fn validate_spec_artifact_extensions(
    spec: &ToolSpec,
    params: &Value,
    emits_json: bool,
    staged: &[StagedArtifact],
) -> Result<(), ToolError> {
    if emits_json {
        return validate_artifact_extensions("json", staged);
    }

    for item in staged {
        let Some(reported_fmt) = normalize_tool_format(&item.format) else {
            return Err(ToolError::EngineFailure(format!(
                "artifact '{}' reported unrecognized format '{}'",
                item.staged_path.display(),
                item.format
            )));
        };
        if !spec.allows_artifact_format(reported_fmt, params) {
            return Err(ToolError::EngineFailure(format!(
                "artifact '{}' reported format '{}', which is not allowed for tool '{}'",
                item.staged_path.display(),
                item.format,
                spec.id
            )));
        }

        let staged_ext = item
            .staged_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if normalize_tool_format(staged_ext) != Some(reported_fmt) {
            return Err(ToolError::EngineFailure(format!(
                "staged artifact '{}' extension does not match reported format '{reported_fmt}'",
                item.staged_path.display(),
            )));
        }

        validate_output_path_extension(Path::new(&item.target_name), reported_fmt)?;
    }

    Ok(())
}

/// File extensions of executable, script, shortcut and installer types. Extracted attachments with
/// one of these as their final extension are saved with `.bin` appended to their file name.
/// `svg`, `html` and `htm` are intentionally absent: they are ordinary document formats.
const RISKY_EXTRACTED_EXTENSIONS: &[&str] = &[
    "ade",
    "adp",
    "apk",
    "app",
    "appimage",
    "applescript",
    "appx",
    "appxbundle",
    "bash",
    "bat",
    "chm",
    "cjs",
    "cmd",
    "com",
    "command",
    "cpl",
    "csh",
    "deb",
    "desktop",
    "dll",
    "dmg",
    "dylib",
    "exe",
    "fileloc",
    "fish",
    "gadget",
    "hta",
    "img",
    "inetloc",
    "ipa",
    "iso",
    "jar",
    "jnlp",
    "js",
    "jse",
    "ksh",
    "lnk",
    "mjs",
    "msc",
    "msi",
    "msix",
    "msixbundle",
    "msp",
    "mst",
    "php",
    "pif",
    "pkg",
    "pl",
    "ps1",
    "ps1xml",
    "psd1",
    "psm1",
    "py",
    "rb",
    "reg",
    "rpm",
    "run",
    "scf",
    "scpt",
    "scr",
    "sh",
    "so",
    "terminal",
    "url",
    "vb",
    "vbe",
    "vbs",
    "vhd",
    "vhdx",
    "webloc",
    "workflow",
    "ws",
    "wsc",
    "wsf",
    "wsh",
    "zsh",
];

/// Suffix appended to the file name of a quarantined extracted attachment.
const QUARANTINE_SUFFIX: &str = ".bin";

fn is_valid_extension_shape(ext: &str) -> bool {
    let lower = ext.trim();
    !lower.is_empty()
        && lower.len() <= 16
        && lower
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Returns `true` if `ext` (without the leading dot, matched case-insensitively) belongs to an
/// executable, script, shortcut or installer file type.
#[must_use]
pub fn is_risky_extracted_extension(ext: &str) -> bool {
    let lower = ext.trim().to_ascii_lowercase();
    RISKY_EXTRACTED_EXTENSIONS.contains(&lower.as_str())
}

/// Returns `true` if the final extension of `file_name` is risky (see
/// [`is_risky_extracted_extension`]). Trailing dots and spaces are ignored, so `run.exe. ` and
/// `report.pdf.exe` are both caught, and matching is case-insensitive.
#[must_use]
pub fn is_risky_extracted_name(file_name: &str) -> bool {
    let trimmed = file_name.trim_end_matches(['.', ' ']);
    trimmed
        .rsplit_once('.')
        .is_some_and(|(_, ext)| is_risky_extracted_extension(ext))
}

/// Returns `true` if `ext` is a safe non-executable file extension permitted for extracted
/// embedded attachments (`pdf.attachments` / `pdf.portfolio` `extract`).
#[must_use]
pub fn is_safe_extracted_extension(ext: &str) -> bool {
    is_valid_extension_shape(ext) && !is_risky_extracted_extension(ext)
}

/// Returns the quarantined target file name (original name plus `.bin`) when `item` is an
/// extracted attachment of a risky type, and `None` when it is published under its own name.
fn quarantined_target_name(item: &StagedArtifact) -> Option<String> {
    if Format::from_user_name(&item.format).is_some() || !is_risky_extracted_name(&item.target_name)
    {
        return None;
    }
    let base = item.target_name.trim_end_matches(['.', ' ']);
    Some(format!("{base}{QUARANTINE_SUFFIX}"))
}

/// One warning per staged extracted attachment that [`publish_atomic`] saves under a `.bin` file
/// name because its type is an executable, script, shortcut or installer.
#[must_use]
pub fn extracted_quarantine_warnings(staged: &[StagedArtifact]) -> Vec<String> {
    staged
        .iter()
        .filter_map(|item| {
            quarantined_target_name(item).map(|renamed| {
                format!(
                    "extracted attachment '{}' is an executable, script, shortcut or installer \
                     type; it was saved as '{renamed}'",
                    item.target_name
                )
            })
        })
        .collect()
}

/// Validates staged artifacts produced by attachment/portfolio extraction, where each extracted
/// file carries its own sanitized extension.
pub fn validate_extracted_artifact_extensions(staged: &[StagedArtifact]) -> Result<(), ToolError> {
    for item in staged {
        let staged_ext = item
            .staged_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let target_ext = Path::new(&item.target_name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        if let Some(expected_fmt) = Format::from_user_name(&item.format) {
            if Format::from_user_name(staged_ext) != Some(expected_fmt) {
                return Err(ToolError::EngineFailure(format!(
                    "staged artifact '{}' extension does not match expected format '{}'",
                    item.staged_path.display(),
                    expected_fmt.user_name()
                )));
            }
            validate_output_path_extension(Path::new(&item.target_name), &item.format)?;
        } else if is_valid_extension_shape(&item.format) {
            if !staged_ext.eq_ignore_ascii_case(&item.format) {
                return Err(ToolError::EngineFailure(format!(
                    "staged artifact '{}' extension '{}' does not match format '{}'",
                    item.staged_path.display(),
                    staged_ext,
                    item.format
                )));
            }
            if !target_ext.eq_ignore_ascii_case(&item.format) {
                return Err(ToolError::Confinement(format!(
                    "output path '{}' extension does not match extracted format '{}'",
                    item.target_name, item.format
                )));
            }
        } else {
            return Err(ToolError::EngineFailure(format!(
                "unsupported extracted artifact format '{}'",
                item.format
            )));
        }
    }

    Ok(())
}

/// A single artifact staged inside `<workspace>/out/` awaiting atomic promotion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedArtifact {
    /// Path to the staged file inside `<workspace>/out/`.
    pub staged_path: PathBuf,
    /// Output format identifier (e.g. `"pdf"`, `"json"`, `"png"`).
    pub format: String,
    /// Target filename (single normal path component) inside the destination directory.
    pub target_name: String,
}

impl StagedArtifact {
    /// Creates a new [`StagedArtifact`].
    #[must_use]
    pub fn new(
        staged_path: impl Into<PathBuf>,
        format: impl Into<String>,
        target_name: impl Into<String>,
    ) -> Self {
        Self {
            staged_path: staged_path.into(),
            format: format.into(),
            target_name: target_name.into(),
        }
    }
}

/// A published artifact in the final destination directory (`docs/tools.md` §12).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublishedArtifact {
    /// Final published path on disk.
    pub path: PathBuf,
    /// Output format identifier (e.g. `"pdf"`, `"json"`).
    pub format: String,
    /// Size of the published file in bytes.
    pub bytes: u64,
}

/// Error returned during output confinement or atomic multi-artifact publication.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PublishError {
    /// Output path resolves to one of the input files (`exit_code = 2`).
    #[error("the output path cannot be the same as an input file")]
    DestinationSameAsInput,
    /// Output path violates confinement (`..`, symlink, hidden component, or outside allowed roots; `exit_code = 2`).
    #[error("output path confinement violation: {0}")]
    Confinement(String),
    /// Destination file already exists and `overwrite` is `false` (`exit_code = 6`).
    #[error("the destination '{0}' already exists; pass --overwrite to replace it")]
    DestinationExists(String),
    /// Staged artifact in `<workspace>/out/` was missing, outside the workspace, or not a regular file (`exit_code = 1`).
    #[error("invalid staged artifact: {0}")]
    InvalidStagedArtifact(String),
    /// Naming template failed validation (`exit_code = 9`).
    #[error("{0}")]
    NamingTemplate(#[from] NamingTemplateError),
    /// Deterministic fault injected on artifact `index` during atomic publication tests (`exit_code = 1`).
    #[error("injected publication failure at artifact index {index}")]
    InjectedFailure {
        /// 0-based artifact index where failure was injected.
        index: usize,
    },
    /// Filesystem I/O failure during promotion (`exit_code = 1`).
    #[error("could not publish output artifact: {0}")]
    Io(String),
}

/// Returns the normative default filename suffix for a tool ID (`docs/tools.md` §6).
///
/// For example:
/// - `"pdf.merge"` -> `"merged"`
/// - `"pdf.rotate"` -> `"rotated"`
/// - `"pdf.compress"` -> `"compressed"`
/// - `"pdf.ocr"` -> `"ocr"`
/// - `"pdf.redact"` -> `"redacted"`
/// - `"pdf.info"` -> `"info"`
#[must_use]
pub fn default_tool_suffix(tool_id: &str) -> String {
    match tool_id {
        "pdf.merge" => "merged".to_owned(),
        "pdf.rotate" => "rotated".to_owned(),
        "pdf.rotate_content" => "rotated-content".to_owned(),
        "pdf.extract_pages" => "extracted".to_owned(),
        "pdf.delete_pages" => "trimmed".to_owned(),
        "pdf.reorder" => "reordered".to_owned(),
        "pdf.crop" | "pdf.crop_whitespace" => "cropped".to_owned(),
        "pdf.scale" => "scaled".to_owned(),
        "pdf.nup" => "nup".to_owned(),
        "pdf.booklet" => "booklet".to_owned(),
        "pdf.single_page" => "single-page".to_owned(),
        "pdf.insert_blank" => "inserted".to_owned(),
        "pdf.alternate_mix" => "mixed".to_owned(),
        "pdf.divide_pages" => "divided".to_owned(),
        "pdf.posterize" => "posterized".to_owned(),
        "pdf.overlay" => "overlay".to_owned(),
        "pdf.metadata" => "metadata".to_owned(),
        "pdf.outline" => "outline".to_owned(),
        "pdf.attachments" => "attachments".to_owned(),
        "pdf.page_labels" => "labeled".to_owned(),
        "pdf.viewer_prefs" => "prefs".to_owned(),
        "pdf.portfolio" => "portfolio".to_owned(),
        "pdf.layers" => "layers".to_owned(),
        "pdf.decompress" => "decompressed".to_owned(),
        "pdf.compress" => "compressed".to_owned(),
        "pdf.ocr" => "ocr".to_owned(),
        "pdf.redact" => "redacted".to_owned(),
        "pdf.info" => "info".to_owned(),
        "pdf.decrypt" => "decrypted".to_owned(),
        "pdf.encrypt" => "encrypted".to_owned(),
        "pdf.linearize" => "linearized".to_owned(),
        "pdf.repair" => "repaired".to_owned(),
        "pdf.flatten" => "flattened".to_owned(),
        "pdf.sign" => "signed".to_owned(),
        "pdf.split" => "part".to_owned(),
        "pdf.sanitize" => "sanitized".to_owned(),
        other => other
            .split_once('.')
            .map_or(other, |(_, action)| action)
            .replace('_', "-"),
    }
}

/// Computes the default single-artifact filename `<stem>.<tool-suffix>.<ext>` (`docs/tools.md` §6).
#[must_use]
pub fn default_single_artifact_name(input_path: &Path, tool_id: &str, ext: &str) -> String {
    let stem = sanitize_stem(
        input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("document"),
    );
    let suffix = default_tool_suffix(tool_id);
    let clean_ext = ext.trim_start_matches('.');
    format!("{stem}.{suffix}.{clean_ext}")
}

/// Computes the default multi-artifact filename `<stem>.<suffix>-<NNN>.<ext>` (`docs/tools.md` §6).
#[must_use]
pub fn default_multi_artifact_name(
    input_path: &Path,
    tool_id: &str,
    index_1_based: usize,
    ext: &str,
) -> String {
    let stem = sanitize_stem(
        input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("document"),
    );
    let clean_ext = ext.trim_start_matches('.');
    let label = match tool_id {
        "pdf.split" => "part".to_owned(),
        "pdf.extract_images" => "image".to_owned(),
        "pdf.to_images" | "pdf.render" => "page".to_owned(),
        _ => default_tool_suffix(tool_id),
    };
    format!("{stem}.{label}-{index_1_based:03}.{clean_ext}")
}

/// When an out-of-process tool stages an artifact named `input-0.<rest>`, replaces the staged
/// `input-0` prefix with the sanitized stem of `first_input` so `<stem>.page-001.png`,
/// `<stem>.combined.png`, `<stem>.pages.zip`, `<stem>.thumbnails.json`, `<stem>.image-001.jpg`,
/// etc. preserve their exact suffix.
#[must_use]
pub fn replace_staged_input_stem(staged_path: &Path, first_input: &Path) -> Option<String> {
    let file_name = staged_path.file_name()?.to_str()?;
    let rest = file_name.strip_prefix("input-0.")?;
    if rest.is_empty() {
        return None;
    }
    let stem = sanitize_stem(
        first_input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("document"),
    );
    Some(format!("{stem}.{rest}"))
}

/// Renders a recipe output naming template (`{stem}`, `{n}`, `{date}`) for a 1-based artifact index.
///
/// Honors `SOURCE_DATE_EPOCH` (UTC `YYYY-MM-DD`) for reproducible builds (`docs/tools.md` §7).
pub fn render_naming_template(
    template: &str,
    stem: &str,
    index_1_based: usize,
) -> Result<String, NamingTemplateError> {
    validate_naming_template(template)?;
    let clean_stem = sanitize_stem(stem);
    let date_str = current_utc_date_string();
    let n_str = format!("{index_1_based:03}");

    let rendered = template
        .replace("{stem}", &clean_stem)
        .replace("{n}", &n_str)
        .replace("{date}", &date_str);

    if rendered.trim().is_empty() {
        return Err(NamingTemplateError::Empty);
    }
    if rendered.starts_with('.') || rendered.contains("..") {
        return Err(NamingTemplateError::DisallowedDotSegment { template: rendered });
    }
    Ok(rendered)
}

fn sanitize_stem(raw: &str) -> String {
    let filtered: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ' ' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = filtered.trim_matches('.');
    if trimmed.is_empty() || trimmed.contains("..") {
        "document".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn current_utc_date_string() -> String {
    let ts = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .and_then(|secs| jiff::Timestamp::from_second(secs).ok())
        .unwrap_or_else(jiff::Timestamp::now);
    let zoned = ts.to_zoned(jiff::tz::TimeZone::UTC);
    format!(
        "{:04}-{:02}-{:02}",
        zoned.year(),
        zoned.month(),
        zoned.day()
    )
}

/// Validates that `path` contains no parent-directory (`..`) traversal components.
pub fn ensure_no_parent_traversal(path: &Path) -> Result<(), PublishError> {
    if path.as_os_str().is_empty() {
        return Err(PublishError::Confinement(
            "output path must not be empty".to_owned(),
        ));
    }
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            return Err(PublishError::Confinement(format!(
                "path '{}' must not contain '..' traversal segments",
                path.display()
            )));
        }
    }
    Ok(())
}

/// Validates that `name` is a single normal filename component without path separators or `..`.
pub fn validate_target_filename(name: &str, reject_hidden: bool) -> Result<(), PublishError> {
    if name.is_empty() || name == "." || name == ".." {
        return Err(PublishError::Confinement(format!(
            "invalid target filename {name:?}"
        )));
    }
    if reject_hidden && name.starts_with('.') {
        return Err(PublishError::Confinement(format!(
            "hidden output filename {name:?} is not permitted"
        )));
    }
    let path = Path::new(name);
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => Err(PublishError::Confinement(format!(
            "target filename {name:?} must be a single filename without directory separators"
        ))),
    }
}

/// Validates and opens `destination_dir` as a capability handle, enforcing `allowed_dirs` when present.
pub fn open_confined_output_dir(
    destination_dir: &Path,
    allowed_dirs: Option<&AllowedDirs>,
) -> Result<(PathBuf, Dir), PublishError> {
    let dir_path = if destination_dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        destination_dir
    };
    ensure_no_parent_traversal(dir_path)?;

    // Reject symlinks at the target directory path itself before canonicalizing.
    if let Ok(meta) = fs::symlink_metadata(dir_path) {
        if meta.file_type().is_symlink() {
            return Err(PublishError::Confinement(format!(
                "output directory '{}' must not be a symlink",
                dir_path.display()
            )));
        }
        if !meta.file_type().is_dir() {
            return Err(PublishError::Confinement(format!(
                "output directory '{}' is not a directory",
                dir_path.display()
            )));
        }
    } else if allowed_dirs.is_none() {
        // In unconfined mode, create the output directory if its parent exists and is not a symlink.
        if let Some(parent) = dir_path.parent()
            && !parent.as_os_str().is_empty()
            && let Ok(parent_meta) = fs::symlink_metadata(parent)
            && parent_meta.file_type().is_symlink()
        {
            return Err(PublishError::Confinement(format!(
                "output parent '{}' must not be a symlink",
                parent.display()
            )));
        }
        fs::create_dir_all(dir_path).map_err(|e| PublishError::Io(e.to_string()))?;
    } else {
        return Err(PublishError::Confinement(format!(
            "confined output directory '{}' does not exist",
            dir_path.display()
        )));
    }

    let canon_dir = fs::canonicalize(dir_path)
        .map_err(|e| PublishError::Confinement(format!("cannot resolve output directory: {e}")))?;

    if let Some(allowed) = allowed_dirs {
        // Reject hidden segments and symlinks along the path beneath the allowed root.
        let Some(root) = allowed
            .roots()
            .iter()
            .find(|r| canon_dir.starts_with(&r.canonical_path))
            .map(|r| &r.canonical_path)
        else {
            return Err(PublishError::Confinement(format!(
                "output directory '{}' is outside allowed directories",
                dir_path.display()
            )));
        };
        if let Ok(rel) = canon_dir.strip_prefix(root) {
            for comp in rel.components() {
                if let Component::Normal(seg) = comp
                    && seg.to_string_lossy().starts_with('.')
                {
                    return Err(PublishError::Confinement(
                        "output directory must not traverse hidden directories".to_owned(),
                    ));
                }
            }
        }
    }

    let cap_dir = Dir::open_ambient_dir(&canon_dir, ambient_authority())
        .map_err(|e| PublishError::Io(e.to_string()))?;
    Ok((canon_dir, cap_dir))
}

/// Atomically publishes a batch of [`StagedArtifact`]s from `workspace` into `destination_dir`.
///
/// If promoting artifact `k` of `N` fails (for example due to `DestinationExists`, I/O error, or
/// `fail_at_index == Some(k)`), all artifacts `0..k` created during this batch are removed from
/// `destination_dir` before returning the error.
pub fn publish_atomic(
    workspace: &Workspace,
    staged: &[StagedArtifact],
    destination_dir: &Path,
    inputs: &[PathBuf],
    allowed_dirs: Option<&AllowedDirs>,
    overwrite: bool,
    fail_at_index: Option<usize>,
) -> Result<Vec<PublishedArtifact>, PublishError> {
    if staged.is_empty() {
        return Ok(Vec::new());
    }

    // Reject duplicate final target filenames before touching the destination directory.
    let mut seen_target_names = HashSet::with_capacity(staged.len());
    for item in staged {
        if !seen_target_names.insert(item.target_name.as_str()) {
            return Err(PublishError::InvalidStagedArtifact(format!(
                "duplicate target filename '{}' in publication batch",
                item.target_name
            )));
        }
    }

    let (canon_dest_dir, dest_cap_dir) = open_confined_output_dir(destination_dir, allowed_dirs)?;
    let ws_out = fs::canonicalize(workspace.output_dir())
        .unwrap_or_else(|_| workspace.output_dir().to_path_buf());

    // Pre-validate every staged artifact and destination filename before touching the target dir.
    let mut staged_sizes = Vec::with_capacity(staged.len());
    // Final file name and reported format per artifact; risky extracted attachments are
    // published under a `.bin` name instead of failing the batch.
    let mut finals: Vec<(String, String)> = Vec::with_capacity(staged.len());
    for item in staged {
        ensure_no_parent_traversal(&item.staged_path)?;
        let quarantined_name = quarantined_target_name(item);
        let (final_name, final_format) = match &quarantined_name {
            Some(renamed) => (renamed.clone(), "bin".to_owned()),
            None => (item.target_name.clone(), item.format.clone()),
        };
        let staged_meta = fs::symlink_metadata(&item.staged_path).map_err(|e| {
            PublishError::InvalidStagedArtifact(format!(
                "staged artifact '{}' could not be stat'd: {e}",
                item.staged_path.display()
            ))
        })?;
        if staged_meta.file_type().is_symlink() || !staged_meta.file_type().is_file() {
            return Err(PublishError::InvalidStagedArtifact(format!(
                "staged artifact '{}' must be a regular file",
                item.staged_path.display()
            )));
        }
        let canon_staged = fs::canonicalize(&item.staged_path).map_err(|e| {
            PublishError::InvalidStagedArtifact(format!(
                "staged artifact '{}' could not be canonicalized: {e}",
                item.staged_path.display()
            ))
        })?;
        if !canon_staged.starts_with(&ws_out) {
            return Err(PublishError::InvalidStagedArtifact(format!(
                "staged artifact '{}' escaped workspace output directory",
                item.staged_path.display()
            )));
        }

        validate_target_filename(&final_name, allowed_dirs.is_some())?;

        let target_full_path = canon_dest_dir.join(&final_name);
        for input in inputs {
            if paths_refer_to_same_file(input, &target_full_path) {
                return Err(PublishError::DestinationSameAsInput);
            }
        }

        let staged_ext = item
            .staged_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let target_ext = Path::new(&item.target_name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if let Some(item_fmt) = normalize_tool_format(&item.format) {
            if normalize_tool_format(staged_ext) != Some(item_fmt) {
                return Err(PublishError::InvalidStagedArtifact(format!(
                    "staged artifact '{}' extension does not match format '{item_fmt}'",
                    item.staged_path.display(),
                )));
            }
            if normalize_tool_format(target_ext) != Some(item_fmt) {
                return Err(PublishError::Confinement(format!(
                    "output path '{}' extension does not match target format '{item_fmt}'",
                    item.target_name,
                )));
            }
        } else if is_valid_extension_shape(&item.format) {
            if !staged_ext.eq_ignore_ascii_case(&item.format) {
                return Err(PublishError::InvalidStagedArtifact(format!(
                    "staged artifact '{}' extension does not match format '{}'",
                    item.staged_path.display(),
                    item.format
                )));
            }
            if !target_ext.eq_ignore_ascii_case(&item.format) {
                return Err(PublishError::Confinement(format!(
                    "output path '{}' extension does not match target format '{}'",
                    item.target_name, item.format
                )));
            }
        } else {
            return Err(PublishError::InvalidStagedArtifact(format!(
                "unrecognized staged artifact format '{}'",
                item.format
            )));
        }

        // Reject symlink or directory at destination target path.
        if let Ok(dest_meta) = dest_cap_dir.symlink_metadata(&final_name) {
            if dest_meta.file_type().is_symlink() {
                return Err(PublishError::Confinement(format!(
                    "destination '{}' is a symlink",
                    target_full_path.display()
                )));
            }
            if dest_meta.file_type().is_dir() {
                return Err(PublishError::Confinement(format!(
                    "destination '{}' is a directory",
                    target_full_path.display()
                )));
            }
        }

        staged_sizes.push(staged_meta.len());
        finals.push((final_name, final_format));
    }

    let mut published = Vec::with_capacity(staged.len());
    let mut batch_entries: Vec<BatchEntry> = Vec::with_capacity(staged.len());

    for (idx, ((item, &bytes), (final_name, final_format))) in staged
        .iter()
        .zip(staged_sizes.iter())
        .zip(finals.iter())
        .enumerate()
    {
        if fail_at_index == Some(idx) {
            rollback_batch(workspace, &dest_cap_dir, &batch_entries);
            return Err(PublishError::InjectedFailure { index: idx });
        }

        let target_rel = Path::new(final_name);
        let backup_name = if overwrite {
            match workspace.backup_existing_into(&dest_cap_dir, target_rel) {
                Ok(bkp) => bkp,
                Err(err) => {
                    rollback_batch(workspace, &dest_cap_dir, &batch_entries);
                    return Err(map_workspace_error(err, item, final_name));
                }
            }
        } else {
            None
        };

        match workspace.promote_into(&item.staged_path, &dest_cap_dir, target_rel, overwrite) {
            Ok(()) => {
                batch_entries.push(BatchEntry {
                    target_name: final_name.clone(),
                    backup_name,
                });
                published.push(PublishedArtifact {
                    path: canon_dest_dir.join(final_name),
                    format: final_format.clone(),
                    bytes,
                });
            }
            Err(err) => {
                if let Some(ref bkp) = backup_name {
                    let _ = workspace.restore_backup_into(bkp, &dest_cap_dir, target_rel);
                }
                rollback_batch(workspace, &dest_cap_dir, &batch_entries);
                return Err(map_workspace_error(err, item, final_name));
            }
        }
    }

    for entry in &batch_entries {
        if let Some(ref bkp) = entry.backup_name {
            let _ = workspace.remove_backup(bkp);
        }
    }

    Ok(published)
}

struct BatchEntry {
    target_name: String,
    backup_name: Option<String>,
}

fn map_workspace_error(
    err: WorkspaceError,
    item: &StagedArtifact,
    target_name: &str,
) -> PublishError {
    match err {
        WorkspaceError::DestinationExists => {
            PublishError::DestinationExists(target_name.to_owned())
        }
        WorkspaceError::ArtifactOutsideOutput | WorkspaceError::ArtifactNotFile => {
            PublishError::InvalidStagedArtifact(format!(
                "staged artifact '{}' is outside workspace output directory or not a regular file",
                item.staged_path.display()
            ))
        }
        WorkspaceError::Io(io_err)
        | WorkspaceError::Cleanup(io_err)
        | WorkspaceError::Promotion(io_err) => PublishError::Io(io_err.to_string()),
    }
}

fn rollback_batch(workspace: &Workspace, dest_cap_dir: &Dir, entries: &[BatchEntry]) {
    for entry in entries.iter().rev() {
        if let Some(ref bkp) = entry.backup_name {
            let _ = workspace.restore_backup_into(bkp, dest_cap_dir, Path::new(&entry.target_name));
        } else {
            let _ = dest_cap_dir.remove_file(&entry.target_name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_artifact_extensions_accepts_matching_formats_and_extensions() {
        let staged = vec![
            StagedArtifact::new(PathBuf::from("/work/out/part-001.pdf"), "pdf", "a.pdf"),
            StagedArtifact::new(PathBuf::from("/work/out/part-002.PDF"), "pdf", "b.PDF"),
        ];
        assert!(validate_artifact_extensions("pdf", &staged).is_ok());

        let json_staged = vec![StagedArtifact::new(
            PathBuf::from("/work/out/output.json"),
            "json",
            "report.info.json",
        )];
        assert!(validate_artifact_extensions("json", &json_staged).is_ok());
    }

    #[test]
    fn validate_artifact_extensions_rejects_mismatched_format_and_extensions() {
        // Engine reported unrecognized format `.command`.
        let cmd_staged = vec![StagedArtifact::new(
            PathBuf::from("/work/out/x.rotated.command"),
            "command",
            "x.rotated.command",
        )];
        let err = validate_artifact_extensions("pdf", &cmd_staged).unwrap_err();
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.code(), "engine_failure");

        // Engine reported valid format ("html") that does not match `spec.output_format` ("pdf").
        let html_staged = vec![StagedArtifact::new(
            PathBuf::from("/work/out/x.rotated.html"),
            "html",
            "x.rotated.html",
        )];
        let err = validate_artifact_extensions("pdf", &html_staged).unwrap_err();
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.code(), "engine_failure");

        // Staged file extension inside `<workspace>/out/` disagrees with `spec.output_format`.
        let bad_staged_ext = vec![StagedArtifact::new(
            PathBuf::from("/work/out/x.rotated.command"),
            "pdf",
            "x.rotated.pdf",
        )];
        let err = validate_artifact_extensions("pdf", &bad_staged_ext).unwrap_err();
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.code(), "engine_failure");

        // Explicit `-o` target filename extension disagrees with `spec.output_format`.
        let bad_target_ext = vec![StagedArtifact::new(
            PathBuf::from("/work/out/x.rotated.pdf"),
            "pdf",
            "custom.html",
        )];
        let err = validate_artifact_extensions("pdf", &bad_target_ext).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert_eq!(err.code(), "usage");
    }

    #[test]
    fn risky_extracted_names_match_case_insensitively_with_double_extensions_and_trailing_dots() {
        for risky in [
            "run.exe",
            "RUN.EXE",
            "x.pdf.exe",
            "setup.LNK",
            "tool.py",
            "link.webloc",
            "a.terminal",
            "disk.dmg",
            "app.AppImage",
            "run.exe.",
            "run.exe. ",
            "run.exe...",
            "x.desktop",
            "m.psm1",
            "c.cpl",
            "r.reg",
            "disc.iso",
            "DISC.ISO",
            "image.img",
            "IMAGE.IMG",
            "virtual.vhd",
            "VIRTUAL.VHD",
            "virtual2.vhdx",
            "VIRTUAL2.VHDX",
            "help.chm",
            "HELP.CHM",
            "pkg.msix",
            "PKG.MSIX",
            "bundle.msixbundle",
            "BUNDLE.MSIXBUNDLE",
            "store.appx",
            "STORE.APPX",
            "store.appxbundle",
            "STORE.APPXBUNDLE",
            "report.pdf.iso.",
            "installer.msixbundle. ",
        ] {
            assert!(is_risky_extracted_name(risky), "{risky} must be risky");
        }
        for safe in [
            "a.pdf",
            "a.svg",
            "page.html",
            "page.HTM",
            "notes.txt",
            "archive.tar.gz",
            "x.exe.pdf",
            "noextension",
            "report.bin",
        ] {
            assert!(!is_risky_extracted_name(safe), "{safe} must not be risky");
        }
        for ext in [
            "exe",
            "LNK",
            "iso",
            "ISO",
            "img",
            "IMG",
            "vhd",
            "VHD",
            "vhdx",
            "VHDX",
            "chm",
            "CHM",
            "msix",
            "MSIX",
            "msixbundle",
            "MSIXBUNDLE",
            "appx",
            "APPX",
            "appxbundle",
            "APPXBUNDLE",
        ] {
            assert!(
                !is_safe_extracted_extension(ext),
                "{ext} must not be a safe extracted extension"
            );
        }
        assert!(is_safe_extracted_extension("svg"));
        assert!(is_safe_extracted_extension("html"));
        assert!(is_safe_extracted_extension("htm"));
        assert!(!is_safe_extracted_extension(""));
        assert!(!is_safe_extracted_extension("a b"));
    }

    #[test]
    fn quarantine_appends_bin_only_to_risky_extracted_names_and_warns_per_file() {
        let staged = vec![
            StagedArtifact::new("/w/out/a.run.exe", "exe", "a.run.exe"),
            StagedArtifact::new("/w/out/a.x.svg", "svg", "a.x.svg"),
            StagedArtifact::new("/w/out/a.y.pdf", "pdf", "a.y.pdf"),
            StagedArtifact::new("/w/out/a.z.LNK", "lnk", "a.z.LNK"),
        ];
        let renamed: Vec<Option<String>> = staged.iter().map(quarantined_target_name).collect();
        assert_eq!(
            renamed,
            vec![
                Some("a.run.exe.bin".to_owned()),
                None,
                None,
                Some("a.z.LNK.bin".to_owned()),
            ]
        );
        let warnings = extracted_quarantine_warnings(&staged);
        assert_eq!(warnings.len(), 2);
        assert!(warnings[0].contains("'a.run.exe'") && warnings[0].contains("'a.run.exe.bin'"));
        assert!(warnings[1].contains("'a.z.LNK'") && warnings[1].contains("'a.z.LNK.bin'"));
    }
}

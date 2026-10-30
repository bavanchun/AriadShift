//! Receives "open these files" launches from the OS file manager.
//!
//! Files arrive three ways: as process arguments on a cold launch (Windows Explorer verbs,
//! Linux `.desktop` `%U`, Nautilus and Dolphin entries), as second-instance arguments forwarded
//! by the single-instance plugin, and as macOS Apple Events (`RunEvent::Opened`) from Finder,
//! "Open With" and the bundled Quick Action. Every source goes through the same filter and lands
//! in one pending list. The window is told through [`EVENT`] and drains the list with [`take`]
//! (the `opened_files` command). The window subscribes to [`EVENT`] first and drains once after
//! that, so files that arrive before it listens are kept and handed over at that first drain.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ariad_core::format::Format;

/// Emitted to the main window whenever new files are pending.
pub const EVENT: &str = "open-files";

/// Matches the queue's job limit; anything beyond it could not be queued anyway.
pub const MAX_PENDING: usize = 1000;

/// Files waiting for the window, in arrival order, without duplicates.
struct Inbox(Mutex<Vec<String>>);

impl Inbox {
    const fn new() -> Self {
        Self(Mutex::new(Vec::new()))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<String>> {
        // The list holds plain strings, so a panic elsewhere cannot leave it inconsistent.
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Adds files without duplicates; returns whether anything was added.
    fn queue(&self, paths: Vec<String>) -> bool {
        let mut pending = self.lock();
        let before = pending.len();
        for path in paths {
            if pending.len() == MAX_PENDING {
                break;
            }
            if !pending.contains(&path) {
                pending.push(path);
            }
        }
        pending.len() > before
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.lock())
    }
}

static PENDING: Inbox = Inbox::new();

/// Input formats the OS file manager offers "Convert with AriadShift" for.
///
/// These are the user-facing source formats of the embedded capability graph. JSON inputs stay
/// accepted when passed explicitly, but are not registered: a verb on every `.json` file in the
/// system would be noise.
#[must_use]
pub fn registered_inputs() -> Vec<Format> {
    let edges = &ariad_core::planner::embedded().edges;
    Format::ALL
        .iter()
        .copied()
        .filter(|format| format.is_user_facing() && format.extensions() != ["json"])
        .filter(|format| edges.iter().any(|edge| edge.from == *format))
        .collect()
}

/// Whether the app can take this path as a conversion input.
fn accepts(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .and_then(Format::from_extension)
        .is_some()
        && path.is_file()
}

/// Turns one launch argument into a candidate path.
///
/// `file://` URLs (Linux `%U`) become paths; other URLs, including `ariadshift://` deep links
/// handled by the deep-link plugin, are ignored. Relative paths resolve against `cwd`, which is
/// the launching process's directory for second-instance arguments.
fn candidate(argument: &OsString, cwd: &Path) -> Option<PathBuf> {
    if let Some(text) = argument.to_str()
        && text
            .get(..7)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file://"))
    {
        return url::Url::parse(text).ok()?.to_file_path().ok();
    }
    let path = PathBuf::from(argument);
    Some(if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    })
}

/// Extracts the convertible files from a full argument vector, including the program name.
///
/// Arguments that start with `-` are options (for example macOS `-psn_…` or an autostart flag)
/// and are skipped until a literal `--`, after which every argument is a path. Directories,
/// missing files, unsupported extensions and paths that are not valid UTF-8 (the window works
/// with string paths) are dropped. The result keeps the first occurrence of each path and holds
/// at most [`MAX_PENDING`] entries.
pub fn parse_args<I, S>(args: I, cwd: &Path) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut options = true;
    let paths = args.into_iter().skip(1).filter_map(|argument| {
        let argument = argument.into();
        if options && argument == "--" {
            options = false;
            return None;
        }
        if options && argument.to_str().is_some_and(|text| text.starts_with('-')) {
            return None;
        }
        candidate(&argument, cwd)
    });
    collect(paths)
}

/// Extracts the convertible files from macOS "open documents" URLs.
pub fn parse_urls<'a>(urls: impl IntoIterator<Item = &'a url::Url>) -> Vec<String> {
    collect(
        urls.into_iter()
            .filter(|url| url.scheme() == "file")
            .filter_map(|url| url.to_file_path().ok()),
    )
}

fn collect(paths: impl Iterator<Item = PathBuf>) -> Vec<String> {
    let mut accepted: Vec<String> = Vec::new();
    for path in paths {
        if accepted.len() == MAX_PENDING {
            break;
        }
        if !accepts(&path) {
            continue;
        }
        let Some(text) = path.to_str() else {
            continue;
        };
        if !accepted.iter().any(|known| known == text) {
            accepted.push(text.to_owned());
        }
    }
    accepted
}

/// Drains the pending files in arrival order.
#[must_use]
pub fn take() -> Vec<String> {
    PENDING.take()
}

/// Records files from a launch and notifies the main window.
pub fn deliver<R: tauri::Runtime>(app: &tauri::AppHandle<R>, paths: Vec<String>) {
    use tauri::{Emitter, Manager};
    if !PENDING.queue(paths) {
        return;
    }
    // The window may not be listening yet on a cold launch; the files stay pending until its
    // first drain, which it runs after subscribing to the event.
    let _ = app.emit_to("main", EVENT, ());
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn args(values: &[&str]) -> Vec<String> {
        std::iter::once("ariad-desktop")
            .chain(values.iter().copied())
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn keeps_supported_files_and_resolves_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a.md", "b.PDF", "c.docx", "notes.txt"] {
            fs::write(dir.path().join(name), b"x").unwrap();
        }
        fs::create_dir(dir.path().join("folder.md")).unwrap();
        let absolute = dir.path().join("c.docx").to_str().unwrap().to_owned();
        let parsed = parse_args(
            args(&[
                "a.md",
                "b.PDF",
                &absolute,
                "notes.txt",
                "missing.md",
                "folder.md",
                "a.md",
            ]),
            dir.path(),
        );
        let expected: Vec<String> = ["a.md", "b.PDF", "c.docx"]
            .iter()
            .map(|name| dir.path().join(name).to_str().unwrap().to_owned())
            .collect();
        assert_eq!(parsed, expected);
    }

    #[test]
    fn skips_options_until_separator_and_ignores_the_program_name() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("-dash.md"), b"x").unwrap();
        fs::write(dir.path().join("ariad-desktop.md"), b"x").unwrap();
        let only_options = parse_args(
            [
                "ariad-desktop.md",
                "-psn_0_12345",
                "--minimized",
                "-dash.md",
            ],
            dir.path(),
        );
        assert!(only_options.is_empty());
        let after_separator = parse_args(["prog", "--flag", "--", "-dash.md"], dir.path());
        assert_eq!(
            after_separator,
            [dir.path().join("-dash.md").to_str().unwrap().to_owned()]
        );
    }

    #[test]
    fn converts_file_urls_and_ignores_other_schemes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("with space.html");
        fs::write(&file, b"x").unwrap();
        let url = url::Url::from_file_path(&file).unwrap();
        let parsed = parse_args(
            args(&[
                url.as_str(),
                "ariadshift://open?route=md-html",
                "https://example.com/a.md",
            ]),
            dir.path(),
        );
        assert_eq!(parsed, [file.to_str().unwrap().to_owned()]);
        let deep = url::Url::parse("ariadshift://open?route=md-html").unwrap();
        assert_eq!(
            parse_urls([&url, &deep]),
            [file.to_str().unwrap().to_owned()]
        );
    }

    #[test]
    fn caps_one_launch_at_the_queue_limit() {
        let dir = tempfile::tempdir().unwrap();
        let names: Vec<String> = (0..=MAX_PENDING).map(|n| format!("{n}.md")).collect();
        for name in &names {
            fs::write(dir.path().join(name), b"x").unwrap();
        }
        let parsed = parse_args(
            std::iter::once("prog".to_owned()).chain(names.iter().cloned()),
            dir.path(),
        );
        assert_eq!(parsed.len(), MAX_PENDING);
    }

    #[test]
    fn pending_files_drain_once_without_duplicates() {
        let inbox = Inbox::new();
        assert!(inbox.queue(vec!["/a.md".into(), "/b.md".into()]));
        assert!(!inbox.queue(vec!["/a.md".into()]));
        assert!(inbox.queue(vec!["/c.md".into(), "/b.md".into()]));
        assert_eq!(inbox.take(), ["/a.md", "/b.md", "/c.md"]);
        assert!(inbox.take().is_empty());
        assert!(!inbox.queue(Vec::new()));
        // A file the window already took may be opened again.
        assert!(inbox.queue(vec!["/a.md".into()]));
        assert_eq!(inbox.take(), ["/a.md"]);
    }

    #[test]
    fn pending_list_stops_at_the_queue_limit() {
        let inbox = Inbox::new();
        let paths: Vec<String> = (0..=MAX_PENDING).map(|n| format!("/{n}.md")).collect();
        assert!(inbox.queue(paths[..MAX_PENDING - 1].to_vec()));
        assert!(inbox.queue(paths[MAX_PENDING - 1..].to_vec()));
        assert!(!inbox.queue(vec!["/late.md".into()]));
        assert_eq!(inbox.take().len(), MAX_PENDING);
    }

    #[test]
    fn cold_launch_files_wait_until_the_window_drains_them() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("notes.md"), b"x").unwrap();
        let inbox = Inbox::new();
        // Arguments arrive during setup, before the window has subscribed or drained.
        assert!(inbox.queue(parse_args(args(&["notes.md"]), dir.path())));
        assert!(!inbox.queue(parse_args(args(&["--", "notes.md"]), dir.path())));
        let expected = [dir.path().join("notes.md").to_str().unwrap().to_owned()];
        assert_eq!(inbox.take(), expected);
        assert!(inbox.take().is_empty());
    }

    #[test]
    fn second_instance_files_reach_a_window_that_already_drained() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        fs::write(second.path().join("report.docx"), b"x").unwrap();
        let inbox = Inbox::new();
        // The window subscribed and found nothing on its first drain.
        assert!(inbox.take().is_empty());
        // A second launch forwards its own arguments and working directory.
        let forwarded = parse_args(
            ["/usr/bin/ariad-desktop", "--", "report.docx"],
            second.path(),
        );
        assert!(inbox.queue(forwarded));
        let expected = second.path().join("report.docx");
        assert_eq!(inbox.take(), [expected.to_str().unwrap().to_owned()]);
        // The same relative name means nothing in the first instance's directory.
        assert!(parse_args(["prog", "report.docx"], first.path()).is_empty());
    }

    #[test]
    fn take_drains_the_shared_list() {
        // The only test touching the shared list, so parallel tests cannot interleave with it.
        let _ = take();
        assert!(PENDING.queue(vec!["/shared.md".into()]));
        assert_eq!(take(), ["/shared.md"]);
        assert!(take().is_empty());
    }

    /// The OS integration files are generated from `os-integration/formats.json`; this keeps
    /// that list equal to the formats the host can actually read.
    /// Rewrite it with `ARIAD_UPDATE_OS_INTEGRATION=1`, then run
    /// `node apps/desktop/os-integration/generate.mjs`.
    #[test]
    fn os_integration_formats_match_the_capability_graph() {
        // Written by hand in the repository's JSON style (Biome): expanded objects, inline arrays.
        let quote = |text: &str| serde_json::to_string(text).unwrap();
        let entries: Vec<String> = registered_inputs()
            .into_iter()
            .map(|format| {
                let extensions: Vec<String> =
                    format.extensions().iter().map(|ext| quote(ext)).collect();
                format!(
                    "    {{\n      \"id\": {},\n      \"extensions\": [{}],\n      \"media_type\": {}\n    }}",
                    quote(format.id()),
                    extensions.join(", "),
                    quote(format.media_type()),
                )
            })
            .collect();
        let expected = format!(
            "{{\n  \"note\": {},\n  \"formats\": [\n{}\n  ]\n}}\n",
            quote("Generated from ariad-core by the ariad-desktop file_open tests. Do not edit."),
            entries.join(",\n"),
        );
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../os-integration/formats.json");
        if std::env::var_os("ARIAD_UPDATE_OS_INTEGRATION").is_some() {
            fs::write(&path, &expected).unwrap();
        }
        let actual = fs::read_to_string(&path).unwrap_or_default();
        assert!(
            actual == expected,
            "{} is stale; rerun this test with ARIAD_UPDATE_OS_INTEGRATION=1, then \
             `node apps/desktop/os-integration/generate.mjs`",
            path.display()
        );
    }
}

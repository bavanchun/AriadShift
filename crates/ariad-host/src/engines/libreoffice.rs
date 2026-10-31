//! `ashift __engine libreoffice`: converts Office and ODF documents through a system LibreOffice.
//!
//! LibreOffice is found through `ASHIFT_SOFFICE`, `PATH`, then the standard install locations.
//! Every job runs `soffice --headless --convert-to` with its own user profile inside the job
//! workspace. The profile raises macro security to its highest level, disables macro execution,
//! active content and OLE automation, and turns link updates and update checks off. The process
//! tree runs in its runner process group (Unix) or Job Object (Windows), so cancellation reaches
//! every child.

use std::{
    collections::VecDeque,
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    thread,
    time::{Duration, Instant},
};

use super::common::{self, ConvertPaths, EngineFailure};
use ariad_core::{
    format::Format,
    limits::Limits,
    protocol::{ErrorCode, Event, Request, RouteCapability, ToolAvailability, ToolStatus},
};

/// Oldest supported LibreOffice release line, `(major, minor)`.
pub const VERSION_FLOOR: (u32, u32) = (24, 2);

/// Environment variable naming the `soffice` executable to use.
pub const SOFFICE_ENV: &str = "ASHIFT_SOFFICE";

const VERSION_TIMEOUT: Duration = Duration::from_secs(30);
const TIMEOUT_MARGIN_SECONDS: u64 = 2;
const OUTPUT_TAIL_BYTES: usize = 16 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// The registry settings written into every job profile.
const REGISTRY_MODIFICATIONS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<oor:items xmlns:oor="http://openoffice.org/2001/registry" xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
<item oor:path="/org.openoffice.Office.Common/Security/Scripting"><prop oor:name="MacroSecurityLevel" oor:op="fuse"><value>3</value></prop></item>
<item oor:path="/org.openoffice.Office.Common/Security/Scripting"><prop oor:name="DisableMacrosExecution" oor:op="fuse"><value>true</value></prop></item>
<item oor:path="/org.openoffice.Office.Common/Security/Scripting"><prop oor:name="DisableActiveContent" oor:op="fuse"><value>true</value></prop></item>
<item oor:path="/org.openoffice.Office.Common/Security/Scripting"><prop oor:name="DisableOLEAutomation" oor:op="fuse"><value>true</value></prop></item>
<item oor:path="/org.openoffice.Office.Common/Security/Scripting"><prop oor:name="BlockUntrustedRefererLinks" oor:op="fuse"><value>true</value></prop></item>
<item oor:path="/org.openoffice.Office.Writer/Content/Update"><prop oor:name="Link" oor:op="fuse"><value>2</value></prop></item>
<item oor:path="/org.openoffice.Office.Calc/Content/Update"><prop oor:name="Link" oor:op="fuse"><value>1</value></prop></item>
<item oor:path="/org.openoffice.Office.Jobs/Jobs/org.openoffice.Office.Jobs:Job['UpdateCheck']/Arguments"><prop oor:name="AutoCheckEnabled" oor:op="fuse"><value>false</value></prop></item>
</oor:items>
"#;

/// A located `soffice` executable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Soffice {
    pub path: PathBuf,
    pub version: String,
}

/// Why no usable `soffice` was found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocateError {
    Missing,
    UnsupportedVersion { found: Option<String> },
}

/// Finds a supported `soffice`: `ASHIFT_SOFFICE` when set, otherwise `PATH`, then the standard
/// install locations.
pub fn locate() -> Result<Soffice, LocateError> {
    if let Some(configured) = env::var_os(SOFFICE_ENV).filter(|value| !value.is_empty()) {
        return check(Path::new(&configured));
    }
    let mut unsupported = None;
    for candidate in candidates() {
        match check(&candidate) {
            Ok(soffice) => return Ok(soffice),
            Err(error @ LocateError::UnsupportedVersion { .. }) => {
                unsupported.get_or_insert(error);
            }
            Err(LocateError::Missing) => {}
        }
    }
    Err(unsupported.unwrap_or(LocateError::Missing))
}

fn candidates() -> Vec<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["soffice.exe"]
    } else {
        &["soffice", "libreoffice"]
    };
    let mut paths: Vec<PathBuf> = env::var_os("PATH")
        .map(|path| {
            env::split_paths(&path)
                .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
                .collect()
        })
        .unwrap_or_default();
    paths.extend(standard_paths());
    paths
}

/// Candidate executables for the host's sandbox policy, without running a tool.
pub(crate) fn candidate_paths() -> Vec<PathBuf> {
    match env::var_os(SOFFICE_ENV).filter(|value| !value.is_empty()) {
        Some(path) => vec![PathBuf::from(path)],
        None => candidates(),
    }
}

fn standard_paths() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        vec![PathBuf::from(
            "/Applications/LibreOffice.app/Contents/MacOS/soffice",
        )]
    } else if cfg!(windows) {
        let mut roots: Vec<PathBuf> = ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"]
            .iter()
            .filter_map(env::var_os)
            .map(PathBuf::from)
            .collect();
        roots.push(PathBuf::from(r"C:\Program Files"));
        roots.push(PathBuf::from(r"C:\Program Files (x86)"));
        roots
            .into_iter()
            .map(|root| root.join("LibreOffice").join("program").join("soffice.exe"))
            .collect()
    } else {
        let mut paths = vec![
            PathBuf::from("/usr/lib/libreoffice/program/soffice"),
            PathBuf::from("/usr/lib64/libreoffice/program/soffice"),
            PathBuf::from("/usr/local/lib/libreoffice/program/soffice"),
            PathBuf::from("/snap/bin/libreoffice"),
        ];
        if let Ok(entries) = fs::read_dir("/opt") {
            let mut opt: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(OsStr::to_str)
                        .is_some_and(|name| name.starts_with("libreoffice"))
                })
                .map(|path| path.join("program").join("soffice"))
                .collect();
            opt.sort();
            opt.reverse();
            paths.extend(opt);
        }
        paths
    }
}

fn check(path: &Path) -> Result<Soffice, LocateError> {
    if !path.is_file() {
        return Err(LocateError::Missing);
    }
    let version = read_version(path).ok_or(LocateError::Missing)?;
    match parse_version(&version) {
        Some(parsed) if parsed >= VERSION_FLOOR => Ok(Soffice {
            path: path.to_path_buf(),
            version,
        }),
        _ => Err(LocateError::UnsupportedVersion {
            found: Some(version),
        }),
    }
}

/// Reads the LibreOffice version: `program/version.ini` on Windows (the GUI binary prints
/// nothing), otherwise the output of `soffice --version`.
fn read_version(path: &Path) -> Option<String> {
    if cfg!(windows)
        && let Some(version) = path
            .parent()
            .map(|dir| dir.join("version.ini"))
            .and_then(|ini| fs::read_to_string(ini).ok())
            .and_then(|ini| {
                ini.lines()
                    .find_map(|line| line.trim().strip_prefix("MsiProductVersion="))
                    .map(|value| value.trim().to_owned())
            })
    {
        return Some(version);
    }
    let output = run_with_timeout(
        Command::new(path)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()),
        VERSION_TIMEOUT,
    )?;
    output
        .split_whitespace()
        .find(|token| token.starts_with(|c: char| c.is_ascii_digit()) && token.contains('.'))
        .map(str::to_owned)
}

fn run_with_timeout(command: &mut Command, timeout: Duration) -> Option<String> {
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = thread::spawn(move || {
        let mut text = Vec::new();
        let _ = stdout.by_ref().take(4096).read_to_end(&mut text);
        String::from_utf8_lossy(&text).into_owned()
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(POLL_INTERVAL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    reader.join().ok()
}

/// Parses `major.minor` from a LibreOffice version string such as `26.8.0.3`.
#[must_use]
pub fn parse_version(version: &str) -> Option<(u32, u32)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts
        .next()?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()?;
    Some((major, minor))
}

/// Reads one engine request, emits NDJSON events, and returns the process exit status.
pub fn serve<R: BufRead, W: Write>(input: R, output: W) -> ExitCode {
    common::serve(input, output, execute)
}

fn capabilities() -> Event {
    let (status, version) = match locate() {
        Ok(soffice) => (ToolAvailability::Found, Some(soffice.version)),
        Err(LocateError::UnsupportedVersion { found }) => (ToolAvailability::WrongVersion, found),
        Err(LocateError::Missing) => (ToolAvailability::Missing, None),
    };
    Event::Capabilities {
        engine: super::LIBREOFFICE.id.to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        tool: ToolStatus {
            name: "soffice".to_owned(),
            version,
            status,
        },
        license: super::LIBREOFFICE.license.to_owned(),
        routes: super::LIBREOFFICE
            .routes
            .iter()
            .map(|(input, output)| RouteCapability {
                input: input.id().to_owned(),
                output: output.id().to_owned(),
            })
            .collect(),
        supports_session: false,
        enforces_memory_limit: false,
        models: None,
    }
}

fn execute<W: Write>(request: Request, output: &mut W) -> Result<(), EngineFailure> {
    match request {
        Request::Describe { protocol, .. } => {
            common::emit_capabilities(output, &protocol, capabilities())
        }
        Request::Convert {
            protocol,
            input,
            output: request_output,
            work_dir,
            limits,
            ..
        } => {
            let paths = common::validate_convert_paths(
                &protocol,
                &input,
                &request_output,
                &work_dir,
                &limits,
            )?;
            let route = Format::from_user_name(&input.format)
                .zip(Format::from_user_name(&request_output.format))
                .filter(|&(from, to)| {
                    super::LIBREOFFICE.routes.contains(&(from, to))
                        && input.format == from.id()
                        && request_output.format == to.id()
                })
                .ok_or(EngineFailure::new(
                    ErrorCode::UnsupportedRoute,
                    "LibreOffice does not convert between these formats.",
                ))?;
            let soffice = locate().map_err(|error| match error {
                LocateError::Missing => EngineFailure::new(
                    ErrorCode::ToolMissing,
                    "LibreOffice was not found; install LibreOffice 24.2 or newer or set ASHIFT_SOFFICE.",
                ),
                LocateError::UnsupportedVersion { .. } => EngineFailure::new(
                    ErrorCode::ToolVersion,
                    "LibreOffice must be 24.2 or newer; install a current release or set ASHIFT_SOFFICE.",
                ),
            })?;
            let artifact = convert(&soffice, &paths, route, &limits)?;
            common::emit_success(output, &artifact, route.1.id())
        }
    }
}

/// The `--convert-to` argument for a target format.
fn convert_target(to: Format) -> Option<&'static str> {
    match to {
        Format::Docx => Some("docx:MS Word 2007 XML"),
        Format::Xlsx => Some("xlsx:Calc MS Excel 2007 XML"),
        Format::Pptx => Some("pptx:Impress MS PowerPoint 2007 XML"),
        // LibreOffice picks the Writer, Calc or Impress PDF export from the document type.
        Format::Pdf => Some("pdf"),
        _ => None,
    }
}

fn write_profile(profile_dir: &Path) -> Result<(), EngineFailure> {
    fs::create_dir_all(profile_dir.join("user")).map_err(|_| EngineFailure::io())?;
    fs::write(
        profile_dir.join("user").join("registrymodifications.xcu"),
        REGISTRY_MODIFICATIONS,
    )
    .map_err(|_| EngineFailure::io())
}

fn convert(
    soffice: &Soffice,
    paths: &ConvertPaths,
    (from, to): (Format, Format),
    limits: &Limits,
) -> Result<PathBuf, EngineFailure> {
    let target = convert_target(to).ok_or(EngineFailure::invalid_request())?;
    let failure = || {
        EngineFailure::new(
            ErrorCode::EngineFailure,
            "LibreOffice could not convert the document.",
        )
    };

    // LibreOffice names its output after the input, so the job gets a fixed input name with
    // the extension of the declared format.
    let source_dir = paths.work_dir.join("lo-in");
    let result_dir = paths.work_dir.join("lo-out");
    let profile_dir = paths.work_dir.join("lo-profile");
    let home_dir = paths.work_dir.join("lo-home");
    for dir in [&source_dir, &result_dir, &home_dir] {
        fs::create_dir_all(dir).map_err(|_| EngineFailure::io())?;
    }
    let source = source_dir.join(format!("source.{}", from.user_name()));
    fs::copy(&paths.input, &source).map_err(|_| EngineFailure::io())?;
    write_profile(&profile_dir)?;

    let mut user_installation = OsString::from("-env:UserInstallation=");
    user_installation.push(file_url(&profile_dir));

    let mut command = Command::new(&soffice.path);
    command
        .args([
            "--headless",
            "--invisible",
            "--nologo",
            "--norestore",
            "--nolockcheck",
            "--nodefault",
            "--nofirststartwizard",
        ])
        .arg(&user_installation)
        .arg("--convert-to")
        .arg(target)
        .arg("--outdir")
        .arg(&result_dir)
        .arg(&source)
        .current_dir(&paths.work_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .env("HOME", &home_dir)
        .env("TMPDIR", &paths.work_dir)
        .env("TMP", &paths.work_dir)
        .env("TEMP", &paths.work_dir);
    if let Some(path) = env::var_os("PATH") {
        command.env("PATH", path);
    }
    #[cfg(target_os = "macos")]
    if let Some(ipc) = env::var_os("OSL_SOCKET_PATH") {
        command.env("OSL_SOCKET_PATH", ipc);
    }
    #[cfg(windows)]
    {
        command
            .env("USERPROFILE", &home_dir)
            .env("APPDATA", &home_dir)
            .env("LOCALAPPDATA", &home_dir);
        if let Some(system_root) = env::var_os("SYSTEMROOT") {
            command.env("SYSTEMROOT", system_root);
        }
    }
    // Inherit the engine's group/job: creating a nested group here would let LibreOffice
    // survive when the runner kills the engine on cancellation or its outer deadline.
    let mut child = command.spawn().map_err(|_| {
        EngineFailure::new(
            ErrorCode::ToolMissing,
            "LibreOffice could not be started; install LibreOffice 24.2 or newer or set ASHIFT_SOFFICE.",
        )
    })?;
    let drains = [
        child.stdout.take().map(drain),
        child.stderr.take().map(drain),
    ];

    let timeout = limits
        .timeout_s
        .map(|seconds| Duration::from_secs(seconds.saturating_sub(TIMEOUT_MARGIN_SECONDS).max(1)));
    let deadline = timeout.and_then(|timeout| Instant::now().checked_add(timeout));
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if deadline.is_none_or(|deadline| Instant::now() < deadline) => {
                thread::sleep(POLL_INTERVAL);
            }
            Ok(None) => break None,
            Err(_) => {
                stop(&mut child);
                return Err(EngineFailure::io());
            }
        }
    };
    // The group or job also holds any helper process LibreOffice left behind.
    stop(&mut child);
    for handle in drains.into_iter().flatten() {
        let _ = handle.join();
    }
    let Some(status) = status else {
        return Err(EngineFailure::new(
            ErrorCode::LimitExceeded,
            "LibreOffice exceeded the conversion time limit.",
        ));
    };
    if !status.success() {
        return Err(failure());
    }

    let produced = result_dir.join(format!("source.{}", to.user_name()));
    let produced_ok = fs::symlink_metadata(&produced)
        .is_ok_and(|meta| meta.file_type().is_file() && meta.len() > 0);
    if !produced_ok {
        return Err(failure());
    }
    let artifact = paths
        .output_dir
        .join(format!("document.{}", to.user_name()));
    fs::rename(&produced, &artifact)
        .or_else(|_| fs::copy(&produced, &artifact).map(|_| ()))
        .map_err(|_| EngineFailure::io())?;
    Ok(artifact)
}

fn stop(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Reads a pipe to its end, keeping only a bounded tail, so the child never blocks on output.
fn drain(mut pipe: impl Read + Send + 'static) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut tail = VecDeque::with_capacity(OUTPUT_TAIL_BYTES);
        let mut chunk = [0_u8; 4096];
        while let Ok(count) = pipe.read(&mut chunk) {
            if count == 0 {
                break;
            }
            let excess = (tail.len() + count).saturating_sub(OUTPUT_TAIL_BYTES);
            tail.drain(..excess.min(tail.len()));
            tail.extend(&chunk[..count]);
        }
        String::from_utf8_lossy(tail.make_contiguous()).into_owned()
    })
}

/// Builds a `file://` URL for an absolute path, percent-encoding everything outside the
/// unreserved set.
#[must_use]
pub fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut url = String::from("file://");
    if !text.starts_with('/') {
        url.push('/');
    }
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'.' | b'_' | b'~' | b':') {
            url.push(char::from(byte));
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    url
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{VERSION_FLOOR, file_url, parse_version};

    #[test]
    fn written_profile_disables_writer_and_calc_link_updates() {
        let directory = tempfile::tempdir().unwrap();
        super::write_profile(directory.path()).unwrap();
        let settings =
            std::fs::read_to_string(directory.path().join("user/registrymodifications.xcu"))
                .unwrap();
        for (component, never) in [("Writer", 2), ("Calc", 1)] {
            let item = format!(
                r#"<item oor:path="/org.openoffice.Office.{component}/Content/Update"><prop oor:name="Link" oor:op="fuse"><value>{never}</value></prop></item>"#
            );
            assert!(
                settings.contains(&item),
                "missing link-update policy: {item}"
            );
        }
    }

    #[test]
    fn parses_libreoffice_versions() {
        assert_eq!(parse_version("26.8.0.3"), Some((26, 8)));
        assert_eq!(parse_version("24.2.7.2"), Some((24, 2)));
        assert_eq!(parse_version("7.6.4.1"), Some((7, 6)));
        assert_eq!(parse_version("25.2"), Some((25, 2)));
        assert_eq!(parse_version("26.8.0~beta1"), Some((26, 8)));
        assert_eq!(parse_version("unknown"), None);
        assert!(parse_version("7.6.4.1").unwrap() < VERSION_FLOOR);
        assert!(parse_version("24.2.0.1").unwrap() >= VERSION_FLOOR);
        assert!(parse_version("26.8.0.3").unwrap() >= VERSION_FLOOR);
    }

    #[test]
    fn file_urls_are_absolute_and_percent_encoded() {
        assert_eq!(
            file_url(Path::new("/tmp/ariad shift/lo-profile")),
            "file:///tmp/ariad%20shift/lo-profile"
        );
        assert_eq!(
            file_url(Path::new("/tmp/vườn")),
            "file:///tmp/v%C6%B0%E1%BB%9Dn"
        );
        assert_eq!(
            file_url(Path::new(r"C:\Users\a b\tmp")),
            "file:///C:/Users/a%20b/tmp"
        );
    }
}

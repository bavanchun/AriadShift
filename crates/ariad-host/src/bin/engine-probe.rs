use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    process::{Command, ExitCode, Stdio},
    thread,
    time::Duration,
};

use ariad_core::protocol::{EngineError, ErrorCode, Event, Request};

fn main() -> ExitCode {
    if env::args_os().nth(1).is_some_and(|arg| arg == "__sandbox") {
        return ariad_host::sandbox::launch(env::args_os().skip(2));
    }
    let mut args = env::args().skip(1);
    let Some(operation) = args.next() else {
        return ExitCode::from(2);
    };

    match operation.as_str() {
        "--version" => {
            println!("pandoc 2.19.2");
            ExitCode::SUCCESS
        }
        "echo" => echo_request(),
        "docling" => docling(args.next()),
        "fail-typed" => fail_typed(ExitCode::from(1)),
        "fail-typed-zero" => fail_typed(ExitCode::SUCCESS),
        "ok-but-nonzero" => {
            emit(&ok_result());
            ExitCode::from(7)
        }
        "crash" => {
            let mut stderr = io::stderr().lock();
            stderr.write_all(b"crash-prefix\n").unwrap();
            stderr.write_all(&vec![b'x'; 70 * 1024]).unwrap();
            stderr.write_all(b"tail-marker").unwrap();
            ExitCode::from(9)
        }
        "no-result" => ExitCode::SUCCESS,
        "bad-json" => {
            println!("not-json");
            ExitCode::SUCCESS
        }
        "stray-stdout" => {
            println!("stray line from native library or runtime");
            emit(&ok_result());
            ExitCode::SUCCESS
        }
        "dump-env" => {
            let mut map = std::collections::BTreeMap::new();
            for (key, val) in env::vars() {
                map.insert(key, serde_json::Value::String(val));
            }
            emit(&Event::Result {
                ok: true,
                metrics: Some(map),
                error: None,
            });
            ExitCode::SUCCESS
        }
        "describe-no-memory-limit" => describe_and_convert(false),
        "describe-multiple-capabilities" => {
            let cap = capabilities("probe-multi", false, false);
            emit(&cap);
            emit(&cap);
            emit(&ok_result());
            ExitCode::SUCCESS
        }
        "describe-enforces-memory" => describe_and_convert(true),
        "session" => serve_session(),
        "hog" => {
            let Some(megabytes) = args.next().and_then(|value| value.parse().ok()) else {
                return ExitCode::from(2);
            };
            hog(megabytes)
        }
        "hog-child" => {
            let Some(megabytes) = args.next() else {
                return ExitCode::from(2);
            };
            let Ok(executable) = env::current_exe() else {
                return ExitCode::from(1);
            };
            let child = Command::new(executable)
                .arg("hog")
                .arg(megabytes)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            if child.is_err() {
                return ExitCode::from(1);
            }
            hang()
        }
        "sandbox-check" => sandbox_check(),
        "event-after-result" => {
            emit(&ok_result());
            emit(&Event::Progress {
                stage: "late".to_owned(),
                done: None,
                total: None,
            });
            ExitCode::SUCCESS
        }
        "flood-stdout" => {
            let mut stdout = io::stdout().lock();
            stdout.write_all(&vec![b'x'; 2 * 1024 * 1024]).unwrap();
            stdout.write_all(b"\n").unwrap();
            stdout.flush().unwrap();
            hang()
        }
        "flood-stderr" => {
            let mut stderr = io::stderr().lock();
            let chunk = [b'x'; 8192];
            for _ in 0..(50 * 1024 * 1024 / chunk.len()) {
                stderr.write_all(&chunk).unwrap();
            }
            emit(&ok_result());
            ExitCode::SUCCESS
        }
        "hang" => hang(),
        "spawn-grandchild-and-hang" => {
            let Some(heartbeat) = args.next() else {
                return ExitCode::from(2);
            };
            let executable = match env::current_exe() {
                Ok(executable) => executable,
                Err(_) => return ExitCode::from(1),
            };
            let child = Command::new(executable)
                .arg("heartbeat")
                .arg(heartbeat)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            if child.is_err() {
                return ExitCode::from(1);
            }
            hang()
        }
        "heartbeat" => {
            let Some(path) = args.next() else {
                return ExitCode::from(2);
            };
            heartbeat(path)
        }
        "outside-artifact" => {
            let Some(path) = args.next() else {
                return ExitCode::from(2);
            };
            emit(&Event::Artifact {
                path,
                format: "docx".to_owned(),
            });
            emit(&ok_result());
            ExitCode::SUCCESS
        }
        _ => ExitCode::from(2),
    }
}

fn read_request() -> Option<Request> {
    let mut line = Vec::new();
    BufReader::new(io::stdin().lock())
        .read_until(b'\n', &mut line)
        .ok()?;
    serde_json::from_slice(&line).ok()
}

fn capabilities(engine: &str, enforces_memory_limit: bool, supports_session: bool) -> Event {
    Event::Capabilities {
        engine: engine.to_owned(),
        version: "0.1.0".to_owned(),
        tool: ariad_core::protocol::ToolStatus {
            name: "probe".to_owned(),
            version: Some("0.1.0".to_owned()),
            status: ariad_core::protocol::ToolAvailability::Found,
        },
        license: "MIT".to_owned(),
        routes: vec![],
        enforces_memory_limit,
        supports_session,
        models: None,
    }
}

/// Answers `describe` with the given memory capability and `convert` with a minimal DOCX.
fn describe_and_convert(enforces_memory_limit: bool) -> ExitCode {
    let Some(request) = read_request() else {
        return ExitCode::from(1);
    };
    match request {
        Request::Describe { .. } => {
            let engine = if enforces_memory_limit {
                "probe-memory"
            } else {
                "probe-no-memory"
            };
            emit(&capabilities(engine, enforces_memory_limit, false));
            emit(&ok_result());
            ExitCode::SUCCESS
        }
        Request::Convert { output, .. } => {
            let artifact = std::path::Path::new(&output.dir).join("document.docx");
            if write_minimal_docx(&artifact).is_err() {
                return ExitCode::from(1);
            }
            emit(&Event::Artifact {
                path: artifact.to_string_lossy().into_owned(),
                format: output.format,
            });
            emit(&ok_result());
            ExitCode::SUCCESS
        }
    }
}

/// A session-capable engine. Each job's behaviour comes from `options.probe`.
fn serve_session() -> ExitCode {
    let mut input = BufReader::new(io::stdin().lock());
    let mut session: Option<String> = None;
    let mut jobs = 0_u64;
    loop {
        let mut line = Vec::new();
        match input.read_until(b'\n', &mut line) {
            Ok(0) => return ExitCode::SUCCESS,
            Ok(_) => {}
            Err(_) => return ExitCode::from(1),
        }
        let Ok(request) = serde_json::from_slice::<Request>(&line) else {
            return invalid_request();
        };
        let (session_id, output, options) = match request {
            Request::Describe { .. } if session.is_none() => {
                emit(&capabilities("probe-session", false, true));
                emit(&ok_result());
                return ExitCode::SUCCESS;
            }
            Request::Describe { .. } => return invalid_request(),
            Request::Convert {
                session_id,
                output,
                options,
                ..
            } => (session_id, output, options),
        };
        if session.is_some() && session != session_id {
            return invalid_request();
        }
        jobs += 1;
        let behaviour = options
            .get("probe")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("ok");
        let mut metrics = std::collections::BTreeMap::from([
            (
                "pid".to_owned(),
                serde_json::Value::from(std::process::id()),
            ),
            ("jobs".to_owned(), serde_json::Value::from(jobs)),
        ]);
        if let Ok(value) = env::var("HF_HUB_OFFLINE") {
            metrics.insert("HF_HUB_OFFLINE".to_owned(), value.into());
        }
        let result = Event::Result {
            ok: true,
            metrics: Some(metrics),
            error: None,
        };
        match behaviour {
            "ok" | "stray-after" => {
                let artifact = std::path::Path::new(&output.dir).join("document.docx");
                if fs::write(&artifact, b"probe session document").is_err() {
                    return ExitCode::from(1);
                }
                emit(&Event::Progress {
                    stage: "convert".to_owned(),
                    done: Some(1),
                    total: Some(1),
                });
                emit(&Event::Artifact {
                    path: artifact.to_string_lossy().into_owned(),
                    format: output.format,
                });
                emit(&result);
                if behaviour == "stray-after" {
                    thread::sleep(Duration::from_millis(100));
                    emit(&Event::Progress {
                        stage: "late".to_owned(),
                        done: None,
                        total: None,
                    });
                }
            }
            "fail" => {
                let _ = fail_typed(ExitCode::SUCCESS);
            }
            "limit" => emit(&Event::Result {
                ok: false,
                metrics: None,
                error: Some(EngineError {
                    code: ErrorCode::LimitExceeded,
                    message: "probe exceeded max_pages".to_owned(),
                }),
            }),
            "crash" => return ExitCode::from(9),
            "hang" => hang(),
            "hog" => hog(options
                .get("mb")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(512)),
            // Both lines leave in one write, so the host sees them together.
            "second-result" => emit_together(&[&result, &result]),
            "late-event" => emit_together(&[
                &result,
                &Event::Progress {
                    stage: "late".to_owned(),
                    done: None,
                    total: None,
                },
            ]),
            _ => return ExitCode::from(2),
        }
        if session_id.is_none() {
            return ExitCode::SUCCESS;
        }
        session = session_id;
    }
}

fn invalid_request() -> ExitCode {
    emit(&Event::Result {
        ok: false,
        metrics: None,
        error: Some(EngineError {
            code: ErrorCode::InvalidRequest,
            message: "probe expects one session per process".to_owned(),
        }),
    });
    ExitCode::from(1)
}

/// Allocates and touches `megabytes` MiB, then waits to be killed.
fn hog(megabytes: u64) -> ! {
    let mut blocks = Vec::new();
    let mut seed = 0x9e37_79b9_u32;
    for _ in 0..megabytes {
        let mut block = vec![0_u8; 1024 * 1024];
        // Varying bytes keep compressed-memory systems from shrinking the resident set.
        for page in block.chunks_mut(4096) {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            page.fill(seed.to_le_bytes()[0] | 1);
        }
        blocks.push(block);
        thread::sleep(Duration::from_millis(2));
    }
    std::hint::black_box(&blocks);
    hang()
}

/// Probes what the sandbox allows and reports each attempt as `allowed` or `denied`.
///
/// `options`: `outside` (a file path outside the workspace to create), `read` (a file to read),
/// `port` (a local TCP port to connect to), `unix` (a pathname UNIX socket to connect to).
fn sandbox_check() -> ExitCode {
    let Some(Request::Convert {
        output, options, ..
    }) = read_request()
    else {
        return ExitCode::from(1);
    };
    let verdict =
        |allowed: bool| serde_json::Value::from(if allowed { "allowed" } else { "denied" });
    let option = |name: &str| options.get(name).and_then(serde_json::Value::as_str);
    let mut metrics = std::collections::BTreeMap::new();
    if let Some(path) = option("outside") {
        metrics.insert(
            "write_outside".to_owned(),
            verdict(fs::write(path, b"escape").is_ok()),
        );
    }
    if let Some(path) = option("read") {
        metrics.insert(
            "read_install_root".to_owned(),
            verdict(fs::read(path).is_ok()),
        );
    }
    if let Some(port) = options.get("port").and_then(serde_json::Value::as_u64) {
        let address =
            std::net::SocketAddr::from(([127, 0, 0, 1], u16::try_from(port).unwrap_or(0)));
        let connected =
            std::net::TcpStream::connect_timeout(&address, Duration::from_secs(2)).is_ok();
        metrics.insert("tcp_connect".to_owned(), verdict(connected));
        let udp = std::net::UdpSocket::bind(("127.0.0.1", 0)).is_ok();
        metrics.insert("udp_socket".to_owned(), verdict(udp));
    }
    #[cfg(unix)]
    if let Some(path) = option("unix") {
        let connected = std::os::unix::net::UnixStream::connect(path).is_ok();
        metrics.insert("unix_connect".to_owned(), verdict(connected));
    }
    let exec_self = env::current_exe()
        .and_then(|executable| Command::new(executable).arg("--version").output())
        .is_ok_and(|output| output.status.success());
    metrics.insert("exec_self".to_owned(), verdict(exec_self));

    let artifact = std::path::Path::new(&output.dir).join("document.docx");
    metrics.insert(
        "write_workspace".to_owned(),
        verdict(fs::write(&artifact, b"inside").is_ok()),
    );
    emit(&Event::Artifact {
        path: artifact.to_string_lossy().into_owned(),
        format: output.format,
    });
    emit(&Event::Result {
        ok: true,
        metrics: Some(metrics),
        error: None,
    });
    ExitCode::SUCCESS
}

fn emit_together(events: &[&Event]) {
    let mut buffer = Vec::new();
    for event in events {
        serde_json::to_writer(&mut buffer, event).unwrap();
        buffer.push(b'\n');
    }
    let mut stdout = io::stdout().lock();
    stdout.write_all(&buffer).unwrap();
    stdout.flush().unwrap();
}

/// Stands in for the Docling engine: `describe` reports a ready engine, and `convert` copies
/// the DoclingDocument JSON fixture named on the command line into the output directory, or
/// reports a missing tool when the fixture is `-`.
fn docling(fixture: Option<String>) -> ExitCode {
    let mut line = Vec::new();
    if BufReader::new(io::stdin().lock())
        .read_until(b'\n', &mut line)
        .is_err()
    {
        return ExitCode::from(1);
    }
    let Ok(request) = serde_json::from_slice::<Request>(&line) else {
        return ExitCode::from(1);
    };
    match request {
        Request::Describe { .. } => {
            emit(&Event::Capabilities {
                engine: "docling".to_owned(),
                version: "2.142.0".to_owned(),
                tool: ariad_core::protocol::ToolStatus {
                    name: "tesseract".to_owned(),
                    version: Some("5.5.3".to_owned()),
                    status: ariad_core::protocol::ToolAvailability::Found,
                },
                license: "MIT".to_owned(),
                routes: vec![],
                enforces_memory_limit: true,
                supports_session: false,
                models: Some(vec!["docling-layout-heron".to_owned()]),
            });
            emit(&ok_result());
        }
        Request::Convert { output, .. } => {
            let Some(fixture) = fixture.filter(|fixture| fixture != "-") else {
                emit(&Event::Result {
                    ok: false,
                    metrics: None,
                    error: Some(EngineError {
                        code: ErrorCode::ToolMissing,
                        message: "OCR is required but Tesseract 5 was not found on PATH".to_owned(),
                    }),
                });
                return ExitCode::SUCCESS;
            };
            emit(&Event::Progress {
                stage: "convert".to_owned(),
                done: Some(1),
                total: Some(1),
            });
            emit(&Event::Warning {
                code: "ocr_fallback".to_owned(),
                message: "OCR was skipped because Tesseract 5 was not found on PATH".to_owned(),
            });
            let target = std::path::Path::new(&output.dir).join("document.docling.json");
            // `symlink:<path>` publishes a link to the fixture instead of a copy.
            let placed = match fixture.strip_prefix("symlink:") {
                Some(source) => link(source, &target),
                None => fs::copy(&fixture, &target).is_ok(),
            };
            if !placed {
                return ExitCode::from(1);
            }
            emit(&Event::Artifact {
                path: target.to_string_lossy().into_owned(),
                format: "docling+json".to_owned(),
            });
            emit(&ok_result());
        }
    }
    ExitCode::SUCCESS
}

#[cfg(unix)]
fn link(source: &str, target: &std::path::Path) -> bool {
    std::os::unix::fs::symlink(source, target).is_ok()
}

#[cfg(not(unix))]
fn link(_source: &str, _target: &std::path::Path) -> bool {
    false
}

fn echo_request() -> ExitCode {
    let mut line = Vec::new();
    if BufReader::new(io::stdin().lock())
        .read_until(b'\n', &mut line)
        .is_err()
    {
        return ExitCode::from(1);
    }
    let request: Request = match serde_json::from_slice(&line) {
        Ok(request) => request,
        Err(_) => return ExitCode::from(1),
    };
    let (output_dir, output_format) = match request {
        Request::Convert { output, .. } => (output.dir, output.format),
        Request::Describe { .. } => return ExitCode::from(1),
    };
    let artifact = std::path::Path::new(&output_dir).join("document.docx");
    if fs::write(&artifact, b"probe document").is_err() {
        return ExitCode::from(1);
    }
    emit(&Event::Artifact {
        path: artifact.to_string_lossy().into_owned(),
        format: output_format,
    });
    emit(&ok_result());
    ExitCode::SUCCESS
}

fn fail_typed(exit: ExitCode) -> ExitCode {
    emit(&Event::Result {
        ok: false,
        metrics: None,
        error: Some(EngineError {
            code: ErrorCode::ToolMissing,
            message: "probe reports a missing tool".to_owned(),
        }),
    });
    exit
}

fn heartbeat(path: impl AsRef<std::path::Path>) -> ExitCode {
    let mut file = match OpenOptions::new().create(true).append(true).open(path) {
        Ok(file) => file,
        Err(_) => return ExitCode::from(1),
    };
    loop {
        if file.write_all(b".").and_then(|()| file.flush()).is_err() {
            return ExitCode::from(1);
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn ok_result() -> Event {
    Event::Result {
        ok: true,
        metrics: None,
        error: None,
    }
}

fn emit(event: &Event) {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    serde_json::to_writer(&mut stdout, event).unwrap();
    stdout.write_all(b"\n").unwrap();
    stdout.flush().unwrap();
}

fn hang() -> ! {
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}

fn write_minimal_docx(path: &std::path::Path) -> io::Result<()> {
    let file = fs::File::create(path)?;
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file(
        "docProps/core.xml",
        zip::write::SimpleFileOptions::default(),
    )
    .map_err(io::Error::other)?;
    zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <dcterms:created xsi:type="dcterms:W3CDTF">2026-10-06T00:00:00Z</dcterms:created>
  <dcterms:modified xsi:type="dcterms:W3CDTF">2026-10-06T00:00:00Z</dcterms:modified>
</cp:coreProperties>"#)?;
    zip.finish().map_err(io::Error::other)?;
    Ok(())
}

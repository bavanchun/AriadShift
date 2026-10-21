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
        "describe-no-memory-limit" => {
            emit(&Event::Capabilities {
                engine: "probe-no-memory".to_owned(),
                version: "0.1.0".to_owned(),
                tool: ariad_core::protocol::ToolStatus {
                    name: "probe".to_owned(),
                    version: Some("0.1.0".to_owned()),
                    status: ariad_core::protocol::ToolAvailability::Found,
                },
                license: "MIT".to_owned(),
                routes: vec![],
                enforces_memory_limit: false,
                models: None,
            });
            emit(&ok_result());
            ExitCode::SUCCESS
        }
        "describe-multiple-capabilities" => {
            let cap = Event::Capabilities {
                engine: "probe-multi".to_owned(),
                version: "0.1.0".to_owned(),
                tool: ariad_core::protocol::ToolStatus {
                    name: "probe".to_owned(),
                    version: Some("0.1.0".to_owned()),
                    status: ariad_core::protocol::ToolAvailability::Found,
                },
                license: "MIT".to_owned(),
                routes: vec![],
                enforces_memory_limit: false,
                models: None,
            };
            emit(&cap);
            emit(&cap);
            emit(&ok_result());
            ExitCode::SUCCESS
        }
        "describe-enforces-memory" => {
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
            match request {
                Request::Describe { .. } => {
                    emit(&Event::Capabilities {
                        engine: "probe-memory".to_owned(),
                        version: "0.1.0".to_owned(),
                        tool: ariad_core::protocol::ToolStatus {
                            name: "probe".to_owned(),
                            version: Some("0.1.0".to_owned()),
                            status: ariad_core::protocol::ToolAvailability::Found,
                        },
                        license: "MIT".to_owned(),
                        routes: vec![],
                        enforces_memory_limit: true,
                        models: None,
                    });
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

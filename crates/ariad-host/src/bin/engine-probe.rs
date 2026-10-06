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
    let artifact = std::path::Path::new(&request.output.dir).join("document.docx");
    if fs::write(&artifact, b"probe document").is_err() {
        return ExitCode::from(1);
    }
    emit(&Event::Artifact {
        path: artifact.to_string_lossy().into_owned(),
        format: request.output.format,
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

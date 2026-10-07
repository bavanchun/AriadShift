use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::io::Write;

fn ashift_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ashift"))
}

fn pandoc_path() -> PathBuf {
    if let Some(path) = std::env::var_os("ASHIFT_PANDOC") {
        return PathBuf::from(path);
    }
    let name = if cfg!(windows) {
        "pandoc.exe"
    } else {
        "pandoc"
    };
    repository_root().join(".tools/pandoc/bin").join(name)
}

#[cfg(unix)]
fn test_capabilities_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("test_capabilities.json")
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of ariad-cli")
        .parent()
        .expect("root repository")
        .to_path_buf()
}

/// Kill-on-drop guard to ensure test processes never leak as orphans (H4).
#[cfg(unix)]
struct ProcessGuard {
    child: Option<Child>,
    engine_pid: Option<u32>,
}

#[cfg(unix)]
impl ProcessGuard {
    fn new(child: Child) -> Self {
        Self {
            child: Some(child),
            engine_pid: None,
        }
    }

    fn set_engine(&mut self, pid: u32) {
        self.engine_pid = Some(pid);
    }
}

#[cfg(unix)]
impl Drop for ProcessGuard {
    fn drop(&mut self) {
        if let Some(pid) = self.engine_pid {
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(unix)]
fn wait_with_timeout(
    child: &mut Child,
    timeout: Duration,
) -> Result<std::process::ExitStatus, String> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(30)),
            Err(e) => return Err(format!("try_wait failed: {e}")),
        }
    }
    Err("process did not exit within timeout".to_string())
}

#[cfg(unix)]
fn is_process_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .output()
        .is_ok_and(|out| out.status.success())
}

#[cfg(unix)]
fn find_hanging_engine(parent_pid: u32, timeout: Duration) -> Option<u32> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Ok(output) = Command::new("ps")
            .args(["-axo", "pid=,ppid=,command="])
            .output()
            && output.status.success()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut procs = Vec::new();
            for line in stdout.lines() {
                let trimmed = line.trim_start();
                if trimmed.is_empty() {
                    continue;
                }
                let mut parts = trimmed.split_whitespace();
                let Some(pid_str) = parts.next() else {
                    continue;
                };
                let Some(ppid_str) = parts.next() else {
                    continue;
                };
                let Ok(pid) = pid_str.parse::<u32>() else {
                    continue;
                };
                let Ok(ppid) = ppid_str.parse::<u32>() else {
                    continue;
                };
                let after_pid = trimmed[pid_str.len()..].trim_start();
                let cmdline = if after_pid.len() >= ppid_str.len() {
                    after_pid[ppid_str.len()..].trim_start()
                } else {
                    ""
                };
                procs.push((pid, ppid, cmdline));
            }

            let mut descendants = std::collections::HashSet::new();
            let mut queue = vec![parent_pid];
            while let Some(current) = queue.pop() {
                for &(pid, ppid, _) in &procs {
                    if ppid == current && descendants.insert(pid) {
                        queue.push(pid);
                    }
                }
            }

            for &(pid, _, cmdline) in &procs {
                if descendants.contains(&pid)
                    && cmdline.contains("__engine")
                    && is_process_alive(pid)
                {
                    // Verify it is alive and sustained (not transient describe child)
                    thread::sleep(Duration::from_millis(60));
                    if is_process_alive(pid) {
                        return Some(pid);
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(30));
    }
    None
}

#[cfg(unix)]
#[test]
fn test_sigterm_cleans_engine_and_workspace() {
    let temp = tempfile::tempdir().expect("create test tempdir");
    let isolated_tmp = temp.path().join("tmp_root");
    fs::create_dir(&isolated_tmp).expect("create isolated tmp");

    let input_file = temp.path().join("doc.md");
    fs::write(
        &input_file,
        "# Test Document\n\nContent for termination test.\n",
    )
    .unwrap();
    let output_file = temp.path().join("doc.docx");

    let child = Command::new(ashift_bin())
        .arg("convert")
        .arg(&input_file)
        .arg("--to")
        .arg("docx")
        .arg("-o")
        .arg(&output_file)
        .env("ASHIFT_PANDOC", pandoc_path())
        .env("ARIAD_TEST_ENGINE", "hang")
        .env("TMPDIR", &isolated_tmp)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ashift convert");

    let ashift_pid = child.id();
    let mut guard = ProcessGuard::new(child);

    let engine_pid = find_hanging_engine(ashift_pid, Duration::from_secs(5));
    assert!(
        engine_pid.is_some(),
        "engine process should have been spawned by ashift"
    );
    let engine_pid = engine_pid.unwrap();
    guard.set_engine(engine_pid);

    assert!(
        is_process_alive(engine_pid),
        "engine process must be running before SIGTERM"
    );

    // Send SIGTERM to ashift
    let status = Command::new("kill")
        .arg("-TERM")
        .arg(ashift_pid.to_string())
        .status()
        .expect("send SIGTERM via kill command");
    assert!(status.success(), "failed to send SIGTERM to ashift");

    // Wait for ashift to exit
    let exit_status = wait_with_timeout(guard.child.as_mut().unwrap(), Duration::from_secs(5))
        .expect("ashift wait failed");
    assert!(
        !exit_status.success(),
        "ashift should not have succeeded after SIGTERM"
    );

    // Verify engine process is dead (poll up to 3 seconds)
    let engine_dead_start = Instant::now();
    let mut engine_alive = true;
    while engine_dead_start.elapsed() < Duration::from_secs(3) {
        if !is_process_alive(engine_pid) {
            engine_alive = false;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !engine_alive,
        "engine process {engine_pid} must be terminated when ashift receives SIGTERM"
    );

    // Verify no workspace directory is left behind in isolated_tmp
    let entries: Vec<_> = fs::read_dir(&isolated_tmp)
        .expect("read isolated_tmp")
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("ariadshift-"))
        .collect();
    assert!(
        entries.is_empty(),
        "workspace directories remained after SIGTERM: {:?}",
        entries.iter().map(|e| e.path()).collect::<Vec<_>>()
    );
}

#[cfg(unix)]
#[test]
fn test_sighup_cleans_engine_and_workspace() {
    let temp = tempfile::tempdir().expect("create test tempdir");
    let isolated_tmp = temp.path().join("tmp_root");
    fs::create_dir(&isolated_tmp).expect("create isolated tmp");

    let input_file = temp.path().join("doc.md");
    fs::write(
        &input_file,
        "# Test Document\n\nContent for termination test.\n",
    )
    .unwrap();
    let output_file = temp.path().join("doc.docx");

    let child = Command::new(ashift_bin())
        .arg("convert")
        .arg(&input_file)
        .arg("--to")
        .arg("docx")
        .arg("-o")
        .arg(&output_file)
        .env("ASHIFT_PANDOC", pandoc_path())
        .env("ARIAD_TEST_ENGINE", "hang")
        .env("TMPDIR", &isolated_tmp)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ashift convert");

    let ashift_pid = child.id();
    let mut guard = ProcessGuard::new(child);

    let engine_pid = find_hanging_engine(ashift_pid, Duration::from_secs(5));
    assert!(
        engine_pid.is_some(),
        "engine process should have been spawned by ashift"
    );
    let engine_pid = engine_pid.unwrap();
    guard.set_engine(engine_pid);

    assert!(
        is_process_alive(engine_pid),
        "engine process must be running before SIGHUP"
    );

    // Send SIGHUP to ashift
    let status = Command::new("kill")
        .arg("-HUP")
        .arg(ashift_pid.to_string())
        .status()
        .expect("send SIGHUP via kill command");
    assert!(status.success(), "failed to send SIGHUP to ashift");

    // Wait for ashift to exit
    let exit_status = wait_with_timeout(guard.child.as_mut().unwrap(), Duration::from_secs(5))
        .expect("ashift wait failed");
    assert!(
        !exit_status.success(),
        "ashift should not have succeeded after SIGHUP"
    );

    // Verify engine process is dead (poll up to 3 seconds)
    let engine_dead_start = Instant::now();
    let mut engine_alive = true;
    while engine_dead_start.elapsed() < Duration::from_secs(3) {
        if !is_process_alive(engine_pid) {
            engine_alive = false;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !engine_alive,
        "engine process {engine_pid} must be terminated when ashift receives SIGHUP"
    );

    // Verify no workspace directory is left behind in isolated_tmp
    let entries: Vec<_> = fs::read_dir(&isolated_tmp)
        .expect("read isolated_tmp")
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("ariadshift-"))
        .collect();
    assert!(
        entries.is_empty(),
        "workspace directories remained after SIGHUP: {:?}",
        entries.iter().map(|e| e.path()).collect::<Vec<_>>()
    );
}

#[cfg(unix)]
#[test]
fn test_mcp_sigterm_exits_promptly() {
    let temp = tempfile::tempdir().expect("create test tempdir");

    let child = Command::new(ashift_bin())
        .arg("mcp")
        .arg("--allow-dir")
        .arg(temp.path())
        .env("ASHIFT_PANDOC", pandoc_path())
        .env("ASHIFT_TEST_CAPABILITIES", test_capabilities_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ashift mcp");

    let ashift_pid = child.id();
    let mut guard = ProcessGuard::new(child);

    // Give server a moment to start
    thread::sleep(Duration::from_millis(100));

    // Send SIGTERM with stdin still open
    let status = Command::new("kill")
        .arg("-TERM")
        .arg(ashift_pid.to_string())
        .status()
        .expect("send SIGTERM via kill command");
    assert!(status.success());

    // Server must exit within 3 seconds even though stdin pipe is open (H2)
    let exit_status = wait_with_timeout(guard.child.as_mut().unwrap(), Duration::from_secs(3))
        .expect("ashift mcp must exit promptly on SIGTERM");
    assert!(
        !exit_status.success(),
        "ashift mcp should exit with failure status on SIGTERM"
    );
}

#[cfg(unix)]
#[test]
fn test_mcp_sighup_exits_promptly() {
    let temp = tempfile::tempdir().expect("create test tempdir");

    let child = Command::new(ashift_bin())
        .arg("mcp")
        .arg("--allow-dir")
        .arg(temp.path())
        .env("ASHIFT_PANDOC", pandoc_path())
        .env("ASHIFT_TEST_CAPABILITIES", test_capabilities_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ashift mcp");

    let ashift_pid = child.id();
    let mut guard = ProcessGuard::new(child);

    // Give server a moment to start
    thread::sleep(Duration::from_millis(100));

    // Send SIGHUP with stdin still open
    let status = Command::new("kill")
        .arg("-HUP")
        .arg(ashift_pid.to_string())
        .status()
        .expect("send SIGHUP via kill command");
    assert!(status.success());

    // Server must exit within 3 seconds even though stdin pipe is open (H2)
    let exit_status = wait_with_timeout(guard.child.as_mut().unwrap(), Duration::from_secs(3))
        .expect("ashift mcp must exit promptly on SIGHUP");
    assert!(
        !exit_status.success(),
        "ashift mcp should exit with failure status on SIGHUP"
    );
}

#[cfg(unix)]
#[test]
fn test_mcp_sigterm_with_hanging_convert_cleans_engine_and_workspace() {
    let temp = tempfile::tempdir().expect("create test tempdir");
    let canonical_temp = fs::canonicalize(temp.path()).unwrap();
    let isolated_tmp = canonical_temp.join("mcp_tmp_root");
    fs::create_dir(&isolated_tmp).expect("create isolated tmp");

    let input_file = canonical_temp.join("mcp_doc.md");
    fs::write(&input_file, "# Document\n\nContent for mcp convert.\n").unwrap();
    let output_file = canonical_temp.join("mcp_doc.docx");

    let mut child = Command::new(ashift_bin())
        .arg("mcp")
        .arg("--allow-dir")
        .arg(&canonical_temp)
        .env("ASHIFT_PANDOC", pandoc_path())
        .env("ASHIFT_TEST_CAPABILITIES", test_capabilities_path())
        .env("ARIAD_TEST_ENGINE", "hang")
        .env("TMPDIR", &isolated_tmp)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ashift mcp");

    let ashift_pid = child.id();
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut guard = ProcessGuard::new(child);

    // Spawn thread to handle roots/list responses and pipe stdin
    let (stdin_tx, stdin_rx) = std::sync::mpsc::channel::<String>();
    thread::spawn(move || {
        while let Ok(msg) = stdin_rx.recv() {
            let _ = writeln!(stdin, "{}", msg);
            let _ = stdin.flush();
        }
    });

    let stdin_writer = stdin_tx.clone();
    thread::spawn(move || {
        use std::io::BufRead;
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) else {
                line.clear();
                continue;
            };
            if val.get("method").and_then(|m| m.as_str()) == Some("roots/list")
                && val.get("id").is_some()
            {
                let resp = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": val.get("id"),
                    "result": { "roots": [] }
                });
                let _ = stdin_writer.send(resp.to_string());
            }
            line.clear();
        }
    });

    // Send initialize request
    let init_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "term-test", "version": "1.0" }
        }
    });
    let _ = stdin_tx.send(init_req.to_string());

    // Give initialize a moment, then send initialized notification
    thread::sleep(Duration::from_millis(100));
    let notif = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
        "params": {}
    });
    let _ = stdin_tx.send(notif.to_string());

    // Send convert tool call
    let convert_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "convert",
            "arguments": {
                "input": input_file.to_str().unwrap(),
                "to": "docx",
                "output": output_file.to_str().unwrap()
            }
        }
    });
    let _ = stdin_tx.send(convert_req.to_string());

    // Find the hanging engine child spawned by ashift
    let engine_pid = find_hanging_engine(ashift_pid, Duration::from_secs(5));
    assert!(
        engine_pid.is_some(),
        "hanging engine process should have been spawned by ashift mcp"
    );
    let engine_pid = engine_pid.unwrap();
    guard.set_engine(engine_pid);
    assert!(is_process_alive(engine_pid), "engine process must be alive");

    // Send SIGTERM to ashift mcp
    let status = Command::new("kill")
        .arg("-TERM")
        .arg(ashift_pid.to_string())
        .status()
        .expect("send SIGTERM");
    assert!(status.success());

    // ashift mcp must exit promptly
    let exit_status = wait_with_timeout(guard.child.as_mut().unwrap(), Duration::from_secs(3))
        .expect("ashift mcp must exit promptly on SIGTERM with hanging engine");
    assert!(!exit_status.success());

    // Verify engine process is killed
    let engine_dead_start = Instant::now();
    let mut engine_alive = true;
    while engine_dead_start.elapsed() < Duration::from_secs(3) {
        if !is_process_alive(engine_pid) {
            engine_alive = false;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !engine_alive,
        "engine process {engine_pid} must be terminated when ashift mcp receives SIGTERM"
    );

    // Verify workspace is cleaned up
    let entries: Vec<_> = fs::read_dir(&isolated_tmp)
        .expect("read isolated_tmp")
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("ariadshift-"))
        .collect();
    assert!(
        entries.is_empty(),
        "workspace directories remained after SIGTERM: {:?}",
        entries.iter().map(|e| e.path()).collect::<Vec<_>>()
    );
}

#[cfg(windows)]
fn find_windows_child_pid(parent_pid: u32, timeout: Duration) -> Option<u32> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        let script = format!(
            "Get-CimInstance Win32_Process | Where-Object {{ $_.ParentProcessId -eq {parent_pid} }} | Select-Object -ExpandProperty ProcessId"
        );
        if let Ok(output) = Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if let Ok(pid) = line.trim().parse::<u32>() {
                    return Some(pid);
                }
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    None
}

#[cfg(windows)]
fn is_windows_process_alive(pid: u32) -> bool {
    let script = format!("Get-Process -Id {pid} -ErrorAction SilentlyContinue");
    Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .map(|out| out.status.success() && !out.stdout.is_empty())
        .unwrap_or(false)
}

#[cfg(windows)]
#[test]
fn test_windows_kill_terminates_engine() {
    let temp = tempfile::tempdir().expect("create test tempdir");
    let input_file = temp.path().join("doc.md");
    fs::write(
        &input_file,
        "# Test Document\n\nContent for termination test.\n",
    )
    .unwrap();
    let output_file = temp.path().join("doc.docx");

    let mut child = Command::new(ashift_bin())
        .arg("convert")
        .arg(&input_file)
        .arg("--to")
        .arg("docx")
        .arg("-o")
        .arg(&output_file)
        .env("ASHIFT_PANDOC", pandoc_path())
        .env("ARIAD_TEST_ENGINE", "hang")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ashift convert");

    let parent_pid = child.id();
    let engine_pid = find_windows_child_pid(parent_pid, Duration::from_secs(5));
    assert!(
        engine_pid.is_some(),
        "engine process should have been spawned"
    );
    let engine_pid = engine_pid.unwrap();
    assert!(
        is_windows_process_alive(engine_pid),
        "engine must be alive initially"
    );

    child.kill().expect("kill ashift");
    let status = child.wait().expect("wait for ashift");
    assert!(!status.success(), "killed process must exit with failure");

    // Assert engine died via KillOnDrop job object
    let start = Instant::now();
    let mut engine_alive = true;
    while start.elapsed() < Duration::from_secs(5) {
        if !is_windows_process_alive(engine_pid) {
            engine_alive = false;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !engine_alive,
        "engine process {engine_pid} must be terminated via Windows Job Object"
    );
    assert!(
        !output_file.exists(),
        "killed process must leave no output file"
    );
}

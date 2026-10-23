use crate::{
    backend::{Backend, directory, validate_route},
    types::*,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime},
};

const MAX_ENTRIES: usize = 10_000;
const MAX_DEPTH: usize = 32;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fingerprint {
    size: u64,
    modified: SystemTime,
}
type Files = HashMap<PathBuf, Fingerprint>;
fn scan_result<T>(result: std::io::Result<T>, skip_unreadable: bool) -> Result<Option<T>, String> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                || (skip_unreadable && error.kind() == std::io::ErrorKind::PermissionDenied) =>
        {
            Ok(None)
        }
        Err(error) => Err(error.to_string()),
    }
}
fn temporary(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    name.starts_with('.')
        || name.starts_with("~$")
        || [".tmp", ".part", ".crdownload"]
            .iter()
            .any(|suffix| lower.ends_with(suffix))
}
fn scan(root: &Path, recursive: bool) -> Result<Files, String> {
    scan_bounded(root, recursive, MAX_ENTRIES)
}
fn scan_bounded(root: &Path, recursive: bool, limit: usize) -> Result<Files, String> {
    let mut files = Files::new();
    let mut stack = vec![(root.to_path_buf(), 0)];
    let mut count = 0;
    while let Some((folder, depth)) = stack.pop() {
        let Some(entries) = scan_result(std::fs::read_dir(&folder), depth > 0)
            .map_err(|error| format!("Cannot scan {}: {error}", folder.display()))?
        else {
            continue;
        };
        for entry in entries {
            let Some(entry) = scan_result(entry, true)? else {
                continue;
            };
            count += 1;
            if count > limit {
                return Err(format!(
                    "Watch folder exceeds the {MAX_ENTRIES} entry limit; choose a smaller folder"
                ));
            }
            if temporary(&entry.file_name().to_string_lossy()) {
                continue;
            }
            let Some(kind) = scan_result(entry.file_type(), true)? else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() && recursive {
                if depth >= MAX_DEPTH {
                    return Err(format!("Watch folder exceeds {MAX_DEPTH} directory levels"));
                }
                stack.push((entry.path(), depth + 1));
            }
            if kind.is_file() {
                let Some(meta) = scan_result(entry.metadata(), true)? else {
                    continue;
                };
                files.insert(
                    entry.path(),
                    Fingerprint {
                        size: meta.len(),
                        modified: meta.modified().map_err(|e| e.to_string())?,
                    },
                );
            }
        }
    }
    Ok(files)
}
struct Poller {
    previous: Files,
    delivered: Files,
}
impl Poller {
    fn new(initial: Files) -> Self {
        Self {
            previous: initial.clone(),
            delivered: initial,
        }
    }
    fn observe(&mut self, current: Files, paused: bool) -> Vec<(PathBuf, Fingerprint)> {
        if paused {
            self.delivered = current.clone();
        }
        self.delivered.retain(|path, _| current.contains_key(path));
        let ready = current
            .iter()
            .filter(|(path, fp)| {
                self.previous.get(*path) == Some(fp) && self.delivered.get(*path) != Some(fp)
            })
            .map(|(path, fp)| (path.clone(), *fp))
            .collect();
        self.previous = current;
        ready
    }
    fn acknowledge(&mut self, path: PathBuf, fp: Fingerprint) {
        self.delivered.insert(path, fp);
    }
}
struct Signal {
    stop: Mutex<bool>,
    wake: Condvar,
    reset: AtomicBool,
}
pub(crate) struct RunningWatch {
    signal: Arc<Signal>,
    thread: Option<JoinHandle<()>>,
}
impl Drop for RunningWatch {
    fn drop(&mut self) {
        *self.signal.stop.lock().unwrap() = true;
        self.signal.wake.notify_all();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
impl Backend {
    pub fn watch(self: &Arc<Self>, request: WatchRequest) -> Result<WatchView, String> {
        let view = self.start_watch(request, false, true)?;
        if let Err(e) = self.persist() {
            let _ = self.watch_control(WatchControl {
                id: view.id.clone(),
                paused: true,
                remove: true,
            });
            return Err(e);
        }
        Ok(view)
    }
    pub(crate) fn start_watch(
        self: &Arc<Self>,
        mut request: WatchRequest,
        paused: bool,
        wait_initial: bool,
    ) -> Result<WatchView, String> {
        validate_route(&request.target)?;
        let from = crate::deep_link::input_format(&request.from)
            .ok_or("Unsupported watch input format")?;
        request.from = from.as_str().into();
        if ariad_host::convert::DocumentFormat::parse(&request.target) == Some(from) {
            return Err("Watch input and output formats must differ".into());
        }
        if !(1..=3600).contains(&request.interval_seconds) {
            return Err("Polling interval must be between 1 and 3600 seconds".into());
        }
        let input = directory(&request.input)?;
        let output = directory(&request.output)?;
        if output.starts_with(&input) {
            return Err("Output folder must be outside the watched folder".into());
        }
        request.input = input.to_string_lossy().into_owned();
        request.output = output.to_string_lossy().into_owned();
        let view = WatchView {
            id: self.id(),
            route: request.clone(),
            paused,
            error: None,
        };
        let mut watches = self.watches.lock().unwrap();
        {
            let mut m = self.model.lock().unwrap();
            if m.watches.len() >= 16 {
                return Err("Watch limit reached (16 folders)".into());
            }
            if m.watches.iter().any(|w| w.route.input == request.input) {
                return Err("This folder is already watched".into());
            }
            m.watches.push(view.clone());
        }
        let weak = Arc::downgrade(self);
        let id = view.id.clone();
        let signal = Arc::new(Signal {
            stop: Mutex::new(false),
            wake: Condvar::new(),
            reset: AtomicBool::new(false),
        });
        let control = signal.clone();
        let (initial_send, initial_recv) = std::sync::mpsc::channel();
        let thread = thread::spawn(move || {
            let initial = scan(&input, request.recursive);
            if let Some(b) = weak.upgrade() {
                set_error(&b, &id, initial.as_ref().err().cloned());
            }
            let _ = initial_send.send(initial.as_ref().map(|_| ()).map_err(Clone::clone));
            let mut poller = initial.ok().map(Poller::new);
            loop {
                let stop = control.stop.lock().unwrap();
                let (stop, _) = control
                    .wake
                    .wait_timeout_while(
                        stop,
                        Duration::from_secs(request.interval_seconds.into()),
                        |stop| !*stop && !control.reset.load(Ordering::Acquire),
                    )
                    .unwrap();
                if *stop {
                    break;
                }
                drop(stop);
                let Some(b) = weak.upgrade() else {
                    break;
                };
                if b.stopped.load(Ordering::Acquire) {
                    break;
                }
                let paused = b
                    .model
                    .lock()
                    .unwrap()
                    .watches
                    .iter()
                    .find(|w| w.id == id)
                    .is_none_or(|w| w.paused);
                let current = match scan(&input, request.recursive) {
                    Ok(current) => current,
                    Err(error) => {
                        set_error(&b, &id, Some(error));
                        continue;
                    }
                };
                if control.reset.swap(false, Ordering::AcqRel) || poller.is_none() {
                    poller = Some(Poller::new(current));
                    set_error(&b, &id, None);
                    continue;
                }
                let poller = poller.as_mut().unwrap();
                let ready = poller.observe(current, paused);
                let mut error = None;
                for (path, fp) in ready {
                    if !ariad_host::convert::detect_format_from_path(&path)
                        .is_some_and(|f| f.as_str() == request.from)
                    {
                        poller.acknowledge(path, fp);
                        continue;
                    }
                    let busy = b.model.lock().unwrap().jobs.iter().any(|j| {
                        j.view.input == path.to_string_lossy()
                            && matches!(j.view.status, JobStatus::Queued | JobStatus::Running)
                    });
                    if busy {
                        continue;
                    }
                    let relative = path.strip_prefix(&input).unwrap();
                    let destination = output.join(relative.parent().unwrap());
                    let result = std::fs::create_dir_all(&destination)
                        .map_err(|e| e.to_string())
                        .and_then(|()| directory(&destination.to_string_lossy()))
                        .and_then(|folder| {
                            if !folder.starts_with(&output) {
                                return Err(
                                    "Watch output directory escapes the chosen folder".into()
                                );
                            }
                            b.enqueue(QueueRequest {
                                paths: vec![path.to_string_lossy().into_owned()],
                                target: request.target.clone(),
                                profile: request.profile,
                                output_folder: Some(folder.to_string_lossy().into_owned()),
                                overwrite: request.overwrite,
                            })
                            .map(|_| ())
                        });
                    match result {
                        Ok(()) => poller.acknowledge(path, fp),
                        Err(e) => error = Some(e),
                    }
                }
                set_error(&b, &id, error);
            }
        });
        watches.insert(
            view.id.clone(),
            RunningWatch {
                signal,
                thread: Some(thread),
            },
        );
        drop(watches);
        // New watches acknowledge their baseline before the caller writes inputs.
        // Saved watches initialize on their thread without blocking application setup.
        if wait_initial {
            if let Err(error) = initial_recv.recv().unwrap_or_else(|e| Err(e.to_string())) {
                self.model
                    .lock()
                    .unwrap()
                    .watches
                    .retain(|w| w.id != view.id);
                self.watches.lock().unwrap().remove(&view.id);
                return Err(error);
            }
        }
        Ok(view)
    }
    pub fn watch_control(&self, request: WatchControl) -> Result<(), String> {
        {
            let mut m = self.model.lock().unwrap();
            let w = m
                .watches
                .iter_mut()
                .find(|w| w.id == request.id)
                .ok_or("Unknown watch folder")?;
            w.paused = request.paused;
            if request.remove {
                m.watches.retain(|w| w.id != request.id);
            }
        }
        let mut watches = self.watches.lock().unwrap();
        if request.remove {
            watches.remove(&request.id);
        } else if let Some(w) = watches.get(&request.id) {
            w.signal.reset.store(true, Ordering::Release);
            w.signal.wake.notify_all();
        }
        drop(watches);
        self.persist()
    }
}
fn set_error(b: &Backend, id: &str, error: Option<String>) {
    if let Some(w) = b
        .model
        .lock()
        .unwrap()
        .watches
        .iter_mut()
        .find(|w| w.id == id)
    {
        w.error = error;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removed_entries_and_unreadable_subfolders_do_not_discard_scan() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deleted.md");
        std::fs::write(&path, "removed during scan").unwrap();
        let entry = std::fs::read_dir(dir.path())
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(scan_result(entry.metadata(), true).unwrap().is_none());
        assert!(
            scan_result::<()>(Err(std::io::ErrorKind::PermissionDenied.into()), true)
                .unwrap()
                .is_none()
        );
        assert!(
            scan_result::<()>(Err(std::io::ErrorKind::PermissionDenied.into()), false).is_err()
        );
        std::fs::write(dir.path().join("good.md"), "readable").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let nested = dir.path().join("unreadable");
            std::fs::create_dir(&nested).unwrap();
            std::fs::set_permissions(&nested, std::fs::Permissions::from_mode(0o000)).unwrap();
            let result = scan(dir.path(), true);
            std::fs::set_permissions(&nested, std::fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(result.unwrap().len(), 1);
        }
        assert_eq!(scan(dir.path(), false).unwrap().len(), 1);
    }
    #[test]
    fn scans_are_bounded_nonrecursive_and_ignore_partial_files() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            ".hidden.md",
            "~$notes.md",
            "a.tmp",
            "a.part",
            "a.crdownload",
            "good.md",
        ] {
            std::fs::write(dir.path().join(name), "data").unwrap();
        }
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        std::fs::write(dir.path().join("nested/deep.md"), "nested").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path().join("good.md"), dir.path().join("link.md")).unwrap();
        assert_eq!(scan(dir.path(), false).unwrap().len(), 1);
        assert_eq!(scan(dir.path(), true).unwrap().len(), 2);
        for i in 0..10 {
            std::fs::write(dir.path().join(format!("file-{i}")), "").unwrap();
        }
        assert!(
            scan_bounded(dir.path(), false, 10)
                .unwrap_err()
                .contains("entry limit")
        );
    }
    #[test]
    fn two_stable_scans_detect_new_modified_and_respect_pause() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        let mut poller = Poller::new(scan(dir.path(), false).unwrap());
        std::fs::write(&path, "first").unwrap();
        assert!(
            poller
                .observe(scan(dir.path(), false).unwrap(), false)
                .is_empty()
        );
        let ready = poller.observe(scan(dir.path(), false).unwrap(), false);
        assert_eq!(ready.len(), 1);
        poller.acknowledge(ready[0].0.clone(), ready[0].1);
        assert!(
            poller
                .observe(scan(dir.path(), false).unwrap(), false)
                .is_empty()
        );
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(3))
            .unwrap();
        assert!(
            poller
                .observe(scan(dir.path(), false).unwrap(), false)
                .is_empty()
        );
        let changed = poller.observe(scan(dir.path(), false).unwrap(), false);
        assert_eq!(changed.len(), 1, "mtime-only changes must be observed");
        poller.acknowledge(changed[0].0.clone(), changed[0].1);
        std::fs::write(&path, "modified text").unwrap();
        assert!(
            poller
                .observe(scan(dir.path(), false).unwrap(), false)
                .is_empty()
        );
        assert_eq!(
            poller
                .observe(scan(dir.path(), false).unwrap(), false)
                .len(),
            1
        );
        poller.observe(scan(dir.path(), false).unwrap(), true);
        assert!(
            poller
                .observe(scan(dir.path(), false).unwrap(), false)
                .is_empty()
        );
        std::fs::remove_file(path).unwrap();
        assert!(
            poller
                .observe(scan(dir.path(), false).unwrap(), false)
                .is_empty()
        );
    }
}

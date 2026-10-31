//! Serializes Windows launches until the single-instance plugin can receive files.
//!
//! Explorer starts one process per selected file. The plugin creates its mutex before
//! its message window, so a second process in between can start another full app.
//! Keep the lock file in place: deleting it would let waiters lock different files.

use std::{
    fs::{File, OpenOptions, TryLockError},
    path::Path,
    time::{Duration, Instant},
};

#[cfg(windows)]
const WAIT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(25);

/// Returns a guard released on drop. An unusable file or timeout permits normal startup.
#[cfg(windows)]
pub(crate) fn acquire(identifier: &str) -> Option<File> {
    acquire_at(
        &std::env::temp_dir().join(format!("{identifier}.launch.lock")),
        WAIT,
    )
}

fn acquire_at(path: &Path, wait: Duration) -> Option<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .ok()?;
    let started = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Some(file),
            Err(TryLockError::WouldBlock) => {
                let remaining = wait.saturating_sub(started.elapsed());
                if remaining.is_zero() {
                    return None;
                }
                std::thread::sleep(POLL.min(remaining));
            }
            Err(TryLockError::Error(_)) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn a_lock_excludes_other_launches_and_releases_on_drop() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("launch.lock");
        let first = acquire_at(&path, Duration::ZERO).unwrap();
        assert!(acquire_at(&path, Duration::ZERO).is_none());
        drop(first);
        assert!(acquire_at(&path, Duration::ZERO).is_some());
        assert!(path.is_file(), "waiters must keep using the same lock file");
    }

    #[test]
    fn a_waiting_launch_proceeds_after_the_first_releases() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("launch.lock");
        let first = acquire_at(&path, Duration::ZERO).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let waiter = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            let guard = acquire_at(&path, Duration::from_secs(5));
            finished_tx.send(guard.is_some()).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            finished_rx.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        drop(first);
        assert!(finished_rx.recv_timeout(Duration::from_secs(5)).unwrap());
        waiter.join().unwrap();
    }

    #[test]
    fn a_busy_gate_stops_waiting_at_the_deadline() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("launch.lock");
        let _first = acquire_at(&path, Duration::ZERO).unwrap();
        let wait = Duration::from_millis(75);
        let started = Instant::now();
        assert!(acquire_at(&path, wait).is_none());
        assert!(started.elapsed() >= wait);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn an_unusable_lock_file_does_not_block_startup() {
        let directory = tempfile::tempdir().unwrap();
        assert!(acquire_at(directory.path(), Duration::from_secs(10)).is_none());
    }
}

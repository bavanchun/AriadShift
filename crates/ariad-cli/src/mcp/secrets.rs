//! Ephemeral secret handles for a local MCP server.
use ariad_core::tools::SecretRef;
use ariad_host::tools::SecretBag;
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

const CAPACITY: usize = 32;
const MAX_BYTES: usize = 4096;
const TTL: Duration = Duration::from_secs(300);

struct Entry {
    handle: String,
    value: String,
    expires: Instant,
}
/// Per-server, single-use secret storage.
pub(super) struct SecretStore {
    entries: Mutex<Vec<Entry>>,
    ttl: Duration,
}
impl Default for SecretStore {
    fn default() -> Self {
        Self::new(TTL)
    }
}
impl SecretStore {
    fn new(ttl: Duration) -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            ttl,
        }
    }
    pub(super) fn put(&self, value: String) -> Result<String, ()> {
        if value.is_empty() || value.len() > MAX_BYTES {
            return Err(());
        }
        let mut entries = self.entries.lock().map_err(|_| ())?;
        let now = Instant::now();
        entries.retain(|entry| entry.expires > now);
        if entries.len() >= CAPACITY {
            return Err(());
        }
        let handle = loop {
            let candidate = format!("sec_{}", uuid::Uuid::new_v4().simple());
            if entries
                .iter()
                .all(|entry| !constant_time_eq(candidate.as_bytes(), entry.handle.as_bytes()))
            {
                break candidate;
            }
        };
        entries.push(Entry {
            handle: handle.clone(),
            value,
            expires: now + self.ttl,
        });
        Ok(handle)
    }
    pub(super) fn take(&self, handles: &[String]) -> Result<SecretBag, ()> {
        let mut entries = self.entries.lock().map_err(|_| ())?;
        let now = Instant::now();
        entries.retain(|entry| entry.expires > now);
        let mut indices = Vec::new();
        for handle in handles {
            if !matches!(SecretRef::handle(handle), Ok(SecretRef::Handle(_))) {
                return Err(());
            }
            let mut found = None;
            for (index, entry) in entries.iter().enumerate() {
                if constant_time_eq(handle.as_bytes(), entry.handle.as_bytes()) {
                    found = Some(index);
                }
            }
            let index = found.ok_or(())?;
            if !indices.contains(&index) {
                indices.push(index);
            }
        }
        indices.sort_unstable_by(|a, b| b.cmp(a));
        let mut bag = SecretBag::new();
        for index in indices {
            let entry = entries.swap_remove(index);
            bag.insert(entry.handle, entry.value);
        }
        Ok(bag)
    }
}
// Handles are generated with a fixed length, and all candidate bytes are compared.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= std::hint::black_box(x ^ y);
    }
    std::hint::black_box(diff) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expiry_capacity_and_single_use() {
        let expired = SecretStore::new(Duration::ZERO);
        let id = expired.put("secret".into()).unwrap();
        assert!(expired.take(&[id]).is_err());
        let store = SecretStore::default();
        let id = store.put("secret".into()).unwrap();
        assert_eq!(
            uuid::Uuid::parse_str(&id[4..]).unwrap().get_version_num(),
            4
        );
        assert_eq!(
            store.take(&[id.clone(), id.clone()]).unwrap().get(&id),
            Some("secret")
        );
        assert!(store.take(&[id]).is_err());
        for _ in 0..CAPACITY {
            store.put("x".into()).unwrap();
        }
        assert!(store.put("x".into()).is_err());
        assert!(store.put("".into()).is_err());
        assert!(store.put("x".repeat(MAX_BYTES + 1)).is_err());
    }
}

#[cfg(test)]
mod concurrency_tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    #[test]
    fn one_concurrent_caller_wins() {
        let store = Arc::new(SecretStore::default());
        let id = store
            .put("quotes \" slash \\ newline\n tiếng Việt".into())
            .unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let threads: Vec<_> = (0..2)
            .map(|_| {
                let store = store.clone();
                let barrier = barrier.clone();
                let id = id.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.take(&[id]).is_ok()
                })
            })
            .collect();
        barrier.wait();
        assert_eq!(
            threads
                .into_iter()
                .map(|t| t.join().unwrap())
                .filter(|won| *won)
                .count(),
            1
        );
    }
    #[test]
    fn invalid_and_missing_handles_are_indistinguishable() {
        let store = SecretStore::default();
        assert!(store.take(&["sec_bad!".into()]).is_err());
        assert!(store.take(&["sec_missing".into()]).is_err());
    }
}

#[cfg(test)]
mod panic_tests {
    #[test]
    fn panic_hook_never_prints_payload() {
        const KEY: &str = "ARIAD_MCP_PANIC_TEST";
        if std::env::var_os(KEY).is_some() {
            super::super::install_panic_hook();
            std::thread::spawn(|| panic!("sensitive-panic-payload"))
                .join()
                .unwrap_err();
            return;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "mcp::secrets::panic_tests::panic_hook_never_prints_payload",
                "--nocapture",
            ])
            .env(KEY, "1")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("sensitive-panic-payload"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("ashift: internal error"));
    }
}

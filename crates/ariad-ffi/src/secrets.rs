//! Out-of-band secrets for tools and recipes.
//!
//! Mirrors the CLI `--secret <name>=...` channel: the app puts each value into a [`SecretBag`]
//! under the tool parameter name (`password`, `user_password`, `api_key`, ...) and passes the bag
//! with the request. Values never appear in `params_json`, outcomes or error messages: the host
//! rejects literal secrets in params and scrubs every resolved value from what it returns.
//! The `sec_<id>` handle store belongs to the MCP server and is not exposed here.

use std::{
    fmt,
    sync::{Arc, Mutex},
};

use ariad_core::tools::SECRET_HANDLE_PREFIX;
use ariad_host::tools::secrets::SecretBag as HostSecretBag;

use crate::FfiError;

/// In-memory secret values for one or more runs. Clear it once the run is done.
#[derive(uniffi::Object)]
pub struct SecretBag {
    inner: Mutex<HostSecretBag>,
}

impl fmt::Debug for SecretBag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The host bag redacts every value and prints only the names.
        match self.inner.lock() {
            Ok(bag) => write!(f, "SecretBag({bag:?})"),
            Err(_) => write!(f, "SecretBag(<poisoned>)"),
        }
    }
}

#[uniffi::export]
impl SecretBag {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(HostSecretBag::new()),
        })
    }

    /// Stores `value` for the tool parameter `name`, replacing any earlier value.
    ///
    /// Rejects reserved names (`prompt` and `sec_` handles) and values the host refuses
    /// (over 4096 bytes, or containing CR, LF or NUL; `certificate` takes up to 14 MiB).
    /// The error never repeats the value.
    pub fn put(&self, name: String, value: String) -> Result<(), FfiError> {
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(FfiError::InvalidRequest {
                message: "secret name must be 1-64 ASCII letters, digits, '_' or '-'".to_string(),
            });
        }
        if name.eq_ignore_ascii_case("prompt") || name.starts_with(SECRET_HANDLE_PREFIX) {
            return Err(FfiError::InvalidRequest {
                message: format!("secret name '{name}' is reserved"),
            });
        }
        let mut bag = self.inner.lock().map_err(|_| FfiError::Internal {
            message: "secret bag lock poisoned".to_string(),
        })?;
        bag.insert(name.clone(), value)
            .map_err(|reason| FfiError::InvalidRequest {
                message: format!("secret '{name}' rejected: {reason}"),
            })
    }

    /// Names of the stored secrets, never their values.
    pub fn names(&self) -> Vec<String> {
        self.inner
            .lock()
            .map(|bag| bag.iter().map(|(k, _)| k.to_string()).collect())
            .unwrap_or_default()
    }

    /// Drops every stored value.
    pub fn clear(&self) {
        if let Ok(mut bag) = self.inner.lock() {
            *bag = HostSecretBag::new();
        }
    }
}

impl SecretBag {
    /// Copy of the stored values for one host request.
    pub(crate) fn snapshot(&self) -> Result<HostSecretBag, FfiError> {
        self.inner
            .lock()
            .map(|bag| bag.clone())
            .map_err(|_| FfiError::Internal {
                message: "secret bag lock poisoned".to_string(),
            })
    }
}

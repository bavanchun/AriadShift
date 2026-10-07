//! Session-scoped resource tracking and `resources/read` implementation.

use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
};

pub use ariad_host::confine::path_to_file_uri;
use ariad_host::confine::{AllowedDirs, confine_input};
use rmcp::{
    ErrorData, RoleServer,
    model::{ReadResourceRequestParams, ReadResourceResult, ResourceContents},
    service::RequestContext,
};

const MAX_SESSION_RESOURCES: usize = 1000;

type ProducedMap = (VecDeque<String>, HashMap<String, PathBuf>);

/// Tracks output resources produced by this server session through `convert`.
#[derive(Clone, Default)]
pub struct SessionResources {
    /// Map from `file://` URI to local canonical filesystem path, bounded with insertion order.
    produced: Arc<Mutex<ProducedMap>>,
}

impl SessionResources {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a newly generated output file in the session resource registry.
    pub fn record(&self, uri: String, path: PathBuf) {
        if let Ok(mut state) = self.produced.lock() {
            let (order, map) = &mut *state;
            if !map.contains_key(&uri) {
                if map.len() >= MAX_SESSION_RESOURCES
                    && let Some(oldest) = order.pop_front()
                {
                    map.remove(&oldest);
                }
                order.push_back(uri.clone());
            }
            map.insert(uri, path);
        }
    }

    /// Looks up a recorded output file by URI.
    pub fn get(&self, uri: &str) -> Option<PathBuf> {
        let state = self.produced.lock().ok()?;
        state.1.get(uri).cloned()
    }
}

/// Determines MIME type from file extension.
#[must_use]
pub fn mime_type_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("md" | "markdown") => "text/markdown; charset=utf-8",
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("epub") => "application/epub+zip",
        _ => "application/octet-stream",
    }
}

/// Handles `resources/read` request for session files.
pub async fn handle_read_resource(
    request: ReadResourceRequestParams,
    _context: RequestContext<RoleServer>,
    resources: &SessionResources,
    allowed_dirs: &Arc<RwLock<AllowedDirs>>,
) -> Result<rmcp::model::ReadResourceResponse, ErrorData> {
    // 1. Verify this URI was produced by this server session
    let local_path = match resources.get(&request.uri) {
        Some(p) => p,
        None => {
            return Err(ErrorData::resource_not_found(
                request.uri,
                Some(serde_json::Value::String(
                    "Resource was not produced by this server session".to_owned(),
                )),
            ));
        }
    };

    // 2. Re-verify path confinement against allowed_dirs at read time and obtain open handle
    let mut confined = {
        let allowed = allowed_dirs
            .read()
            .map_err(|_| ErrorData::internal_error("internal lock error", None))?;

        confine_input(&local_path, &allowed).map_err(|e| {
            ErrorData::invalid_params(
                format!("Access denied for resource {}: {e}", request.uri),
                None,
            )
        })?
    };

    let meta = confined
        .file
        .metadata()
        .map_err(|e| ErrorData::internal_error(format!("failed to stat resource: {e}"), None))?;

    if meta.len() > 32 * 1024 * 1024 {
        return Err(ErrorData::invalid_params(
            format!("resource {} exceeds 32 MiB limit", request.uri),
            None,
        ));
    }

    let mime = mime_type_for_path(&local_path);
    let is_text = mime.starts_with("text/");
    let uri_clone = request.uri.clone();

    let res = tokio::task::spawn_blocking(move || -> Result<ResourceContents, ErrorData> {
        use std::io::{Read, Seek, SeekFrom};
        confined.file.seek(SeekFrom::Start(0)).map_err(|e| {
            ErrorData::internal_error(format!("failed to seek resource: {e}"), None)
        })?;

        let mut bytes = Vec::with_capacity(meta.len() as usize);
        confined.file.read_to_end(&mut bytes).map_err(|e| {
            ErrorData::internal_error(format!("failed to read resource: {e}"), None)
        })?;

        if is_text {
            let text_content = String::from_utf8(bytes).map_err(|e| {
                ErrorData::internal_error(format!("resource is not valid UTF-8: {e}"), None)
            })?;
            Ok(ResourceContents::text(text_content, &uri_clone).with_mime_type(mime))
        } else {
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            Ok(ResourceContents::blob(encoded, &uri_clone).with_mime_type(mime))
        }
    })
    .await
    .map_err(|e| ErrorData::internal_error(format!("task join error: {e}"), None))??;

    Ok(ReadResourceResult::new(vec![res]).into())
}

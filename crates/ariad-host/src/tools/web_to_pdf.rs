//! Host-side execution for the `web.to_pdf` tool.
//!
//! Fetches a web page and its same-origin sub-assets (images, CSS, CSS imports, CSS url references)
//! using an SSRF-safe HTTP client with DNS pinning and public IP enforcement.
//! The fetched page is stored in a confined directory in the workspace and rendered offline
//! via HTML -> IR -> Typst into a PDF document.

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

use ariad_core::{
    format::Format,
    limits::Limits,
    tools::{ToolSpec, params::WebToPdfParams},
};
use cap_std::ambient_authority;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    assets::AssetBase,
    convert::{ConvertWarning, safe_warning},
    net::{Client, NetError},
    tools::{
        inprocess::{HostClock, RawToolExecution},
        run::{ToolError, ToolEvent},
        secrets::SecretScrubber,
    },
    workspace::Workspace,
};

/// Maximum allowed top-level HTML response size in bytes (10 MiB).
pub const MAX_TOP_LEVEL_HTML_BYTES: usize = 10 * 1024 * 1024;
/// Maximum allowed number of sub-assets fetched.
pub const MAX_SUB_ASSET_COUNT: usize = 50;
/// Maximum allowed size for any single sub-asset (20 MiB).
pub const MAX_PER_ASSET_BYTES: usize = 20 * 1024 * 1024;
/// Maximum cumulative bytes across HTML and all sub-assets (64 MiB).
pub const MAX_CUMULATIVE_BYTES: usize = 64 * 1024 * 1024;
/// Overall deadline across the entire `web.to_pdf` job (30 s).
pub const TOTAL_DEADLINE: Duration = Duration::from_secs(30);
/// Maximum timeout cap for any single HTTP request within the job (15 s).
pub const PER_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Preflight check for `web.to_pdf`.
pub fn preflight() -> Result<(), ToolError> {
    Ok(())
}

/// Executes `web.to_pdf` using a production SSRF-safe client.
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute(
    spec: &ToolSpec,
    params_value: &serde_json::Value,
    workspace: &Workspace,
    limits: &Limits,
    scrubber: &SecretScrubber,
    cancel: &CancellationToken,
    on_event: &mut dyn FnMut(ToolEvent),
    clock: HostClock,
) -> Result<RawToolExecution, ToolError> {
    let client = Client::builder()
        .max_response_bytes(MAX_PER_ASSET_BYTES)
        .total_timeout(TOTAL_DEADLINE)
        .https_only(true)
        .allow_loopback(false)
        .build()
        .map_err(|e| ToolError::EngineFailure(e.to_string()))?;

    execute_with_client(
        &client,
        spec,
        params_value,
        workspace,
        limits,
        scrubber,
        cancel,
        on_event,
        clock,
    )
}

/// Discovered reference in HTML or CSS.
#[derive(Clone, Debug, Eq, PartialEq)]
struct RawAssetCandidate {
    raw_reference: String,
    is_css: bool,
}

/// Checks whether HTML text contains a `<script` tag.
fn contains_script_tag(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    let mut rest = lower.as_str();
    while let Some(idx) = rest.find("<script") {
        let after = &rest[idx + 7..];
        if after.starts_with(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/') {
            return true;
        }
        rest = &rest[idx + 7..];
    }
    false
}

/// Extracts attribute values like `src="..."`, `src='...'`, or `src=...` from an HTML tag string.
fn extract_attribute<'a>(tag_str: &'a str, attr_name: &str) -> Option<&'a str> {
    let bytes = tag_str.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'='
            && bytes[i] != b'>'
            && bytes[i] != b'/'
        {
            i += 1;
        }
        let key = &tag_str[start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }
            let val_start;
            let val_end;
            if bytes[i] == b'"' {
                i += 1;
                val_start = i;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += 1;
                }
                val_end = i;
                if i < bytes.len() {
                    i += 1;
                }
            } else if bytes[i] == b'\'' {
                i += 1;
                val_start = i;
                while i < bytes.len() && bytes[i] != b'\'' {
                    i += 1;
                }
                val_end = i;
                if i < bytes.len() {
                    i += 1;
                }
            } else {
                val_start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                    i += 1;
                }
                val_end = i;
            }

            if key.eq_ignore_ascii_case(attr_name) {
                return Some(&tag_str[val_start..val_end]);
            }
        }
    }
    None
}

/// Scans HTML text for sub-asset references (`<img>` `src`, `srcset`, `<source>` `srcset`, and `<link rel="stylesheet">` `href`).
fn scan_html_sub_assets(html: &str) -> Vec<RawAssetCandidate> {
    let mut candidates = Vec::new();
    let bytes = html.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'<' {
            let tag_start = i;
            while i < bytes.len() && bytes[i] != b'>' {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }
            let tag_end = i;
            let tag_slice = &html[tag_start + 1..tag_end];
            let tag_name = tag_slice
                .split_ascii_whitespace()
                .next()
                .unwrap_or("")
                .trim_start_matches('/');

            if tag_name.eq_ignore_ascii_case("img") {
                if let Some(src) = extract_attribute(tag_slice, "src") {
                    let trimmed = src.trim();
                    if !trimmed.is_empty() && !trimmed.starts_with("data:") {
                        candidates.push(RawAssetCandidate {
                            raw_reference: trimmed.to_owned(),
                            is_css: false,
                        });
                    }
                }
                if let Some(srcset) = extract_attribute(tag_slice, "srcset") {
                    for part in srcset.split(',') {
                        let trimmed = part.trim();
                        if let Some(url_part) = trimmed.split_ascii_whitespace().next()
                            && !url_part.is_empty()
                            && !url_part.starts_with("data:")
                        {
                            candidates.push(RawAssetCandidate {
                                raw_reference: url_part.to_owned(),
                                is_css: false,
                            });
                        }
                    }
                }
            } else if tag_name.eq_ignore_ascii_case("source") {
                if let Some(srcset) = extract_attribute(tag_slice, "srcset") {
                    for part in srcset.split(',') {
                        let trimmed = part.trim();
                        if let Some(url_part) = trimmed.split_ascii_whitespace().next()
                            && !url_part.is_empty()
                            && !url_part.starts_with("data:")
                        {
                            candidates.push(RawAssetCandidate {
                                raw_reference: url_part.to_owned(),
                                is_css: false,
                            });
                        }
                    }
                }
            } else if tag_name.eq_ignore_ascii_case("link") {
                let is_stylesheet = extract_attribute(tag_slice, "rel")
                    .is_some_and(|r| r.to_ascii_lowercase().contains("stylesheet"));
                if is_stylesheet && let Some(href) = extract_attribute(tag_slice, "href") {
                    let trimmed = href.trim();
                    if !trimmed.is_empty() && !trimmed.starts_with("data:") {
                        candidates.push(RawAssetCandidate {
                            raw_reference: trimmed.to_owned(),
                            is_css: true,
                        });
                    }
                }
            }
        }
        i += 1;
    }
    candidates
}

/// Scans CSS text for `@import` targets and `url(...)` targets.
fn scan_css_sub_assets(css: &str) -> Vec<RawAssetCandidate> {
    let mut candidates = Vec::new();
    let bytes = css.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // Look for @import
        if i + 7 <= len
            && css
                .get(i..i + 7)
                .is_some_and(|s| s.eq_ignore_ascii_case("@import"))
        {
            let mut j = i + 7;
            while j < len && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < len {
                if bytes[j] == b'"' || bytes[j] == b'\'' {
                    let quote = bytes[j];
                    j += 1;
                    let start = j;
                    while j < len && bytes[j] != quote && bytes[j] != b'\n' && bytes[j] != b';' {
                        j += 1;
                    }
                    let ref_str = &css[start..j];
                    let trimmed = ref_str.trim();
                    if !trimmed.is_empty() && !trimmed.starts_with("data:") {
                        candidates.push(RawAssetCandidate {
                            raw_reference: trimmed.to_owned(),
                            is_css: true,
                        });
                    }
                    i = j;
                    continue;
                } else if j + 4 <= len
                    && css
                        .get(j..j + 4)
                        .is_some_and(|s| s.eq_ignore_ascii_case("url("))
                {
                    j += 4;
                    while j < len && bytes[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    let quote = if j < len && (bytes[j] == b'"' || bytes[j] == b'\'') {
                        let q = bytes[j];
                        j += 1;
                        Some(q)
                    } else {
                        None
                    };
                    let start = j;
                    while j < len {
                        if let Some(q) = quote {
                            if bytes[j] == q {
                                break;
                            }
                        } else if bytes[j] == b')' || bytes[j].is_ascii_whitespace() {
                            break;
                        }
                        j += 1;
                    }
                    let ref_str = &css[start..j];
                    let trimmed = ref_str.trim();
                    if !trimmed.is_empty() && !trimmed.starts_with("data:") {
                        candidates.push(RawAssetCandidate {
                            raw_reference: trimmed.to_owned(),
                            is_css: true,
                        });
                    }
                    i = j;
                    continue;
                }
            }
        }

        // Look for url(...)
        if i + 4 <= len
            && css
                .get(i..i + 4)
                .is_some_and(|s| s.eq_ignore_ascii_case("url("))
        {
            let mut j = i + 4;
            while j < len && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let quote = if j < len && (bytes[j] == b'"' || bytes[j] == b'\'') {
                let q = bytes[j];
                j += 1;
                Some(q)
            } else {
                None
            };
            let start = j;
            while j < len {
                if let Some(q) = quote {
                    if bytes[j] == q {
                        break;
                    }
                } else if bytes[j] == b')' || bytes[j].is_ascii_whitespace() {
                    break;
                }
                j += 1;
            }
            let ref_str = &css[start..j];
            let trimmed = ref_str.trim();
            if !trimmed.is_empty() && !trimmed.starts_with("data:") {
                candidates.push(RawAssetCandidate {
                    raw_reference: trimmed.to_owned(),
                    is_css: false,
                });
            }
            i = j;
            continue;
        }

        i += 1;
    }
    candidates
}

/// Infers a safe file extension from a URL and Content-Type header.
fn infer_extension(url: &Url, content_type: &str, is_css: bool) -> &'static str {
    if is_css {
        return "css";
    }
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        "text/css" => "css",
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        _ => {
            let path = url.path().to_ascii_lowercase();
            if path.ends_with(".png") {
                "png"
            } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
                "jpg"
            } else if path.ends_with(".webp") {
                "webp"
            } else if path.ends_with(".svg") {
                "svg"
            } else if path.ends_with(".css") {
                "css"
            } else if path.ends_with(".bmp") {
                "bmp"
            } else if path.ends_with(".tiff") || path.ends_with(".tif") {
                "tiff"
            } else {
                "png"
            }
        }
    }
}

/// Executes `web.to_pdf` with an explicit SSRF-safe `Client`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_with_client(
    client: &Client,
    spec: &ToolSpec,
    params_value: &serde_json::Value,
    workspace: &Workspace,
    limits: &Limits,
    scrubber: &SecretScrubber,
    cancel: &CancellationToken,
    on_event: &mut dyn FnMut(ToolEvent),
    clock: HostClock,
) -> Result<RawToolExecution, ToolError> {
    execute_with_client_deadline(
        client,
        TOTAL_DEADLINE.min(client.policy().total_timeout),
        spec,
        params_value,
        workspace,
        limits,
        scrubber,
        cancel,
        on_event,
        clock,
    )
}

/// Executes `web.to_pdf` with an explicit SSRF-safe `Client` and an explicit total job deadline.
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_with_client_deadline(
    client: &Client,
    total_deadline: Duration,
    _spec: &ToolSpec,
    params_value: &serde_json::Value,
    workspace: &Workspace,
    limits: &Limits,
    _scrubber: &SecretScrubber,
    cancel: &CancellationToken,
    _on_event: &mut dyn FnMut(ToolEvent),
    clock: HostClock,
) -> Result<RawToolExecution, ToolError> {
    if cancel.is_cancelled() {
        return Err(ToolError::Interrupted);
    }

    let start_time = Instant::now();

    // 1. Parse and validate parameters
    let params: WebToPdfParams = serde_json::from_value(params_value.clone())
        .map_err(|e| ToolError::InvalidParams(e.to_string()))?;
    params.validate().map_err(ToolError::InvalidParams)?;

    let target_url = Url::parse(params.url.trim())
        .map_err(|e| ToolError::InvalidParams(format!("invalid url: {e}")))?;

    let remaining = match total_deadline.checked_sub(start_time.elapsed()) {
        Some(r) if !r.is_zero() => r,
        _ => return Err(ToolError::LimitExceeded),
    };
    let top_req_timeout = remaining.min(client.policy().total_timeout);
    let top_read_timeout = remaining
        .min(client.policy().read_timeout)
        .min(PER_REQUEST_TIMEOUT);
    let top_client = client.with_timeouts(top_req_timeout, top_read_timeout);

    // 2. Fetch top-level HTML page with redirect following
    let (response, final_url) = top_client
        .get_with_final_url(target_url)
        .map_err(map_net_error)?;

    if !response.is_success() {
        return Err(ToolError::EngineFailure(format!(
            "HTTP request returned status {}",
            response.status
        )));
    }

    // 3. Verify top-level Content-Type
    let content_type = response.header("content-type").unwrap_or("");
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if mime != "text/html" && mime != "application/xhtml+xml" {
        return Err(ToolError::EngineFailure(format!(
            "invalid content type '{mime}', expected text/html or application/xhtml+xml"
        )));
    }

    // 4. Verify top-level HTML byte size
    if response.body.len() > MAX_TOP_LEVEL_HTML_BYTES {
        return Err(ToolError::LimitExceeded);
    }

    let mut cumulative_bytes = response.body.len();
    if cumulative_bytes > MAX_CUMULATIVE_BYTES {
        return Err(ToolError::LimitExceeded);
    }

    let final_origin = final_url.origin();
    let mut warnings = Vec::new();

    let html_text = String::from_utf8_lossy(&response.body).to_string();

    // 5. Check for <script> tags
    if contains_script_tag(&html_text) {
        warnings.push(ConvertWarning {
            code: "js_content_not_rendered".to_string(),
            message: "JavaScript is not executed; dynamic content was not rendered".to_string(),
        });
    }

    // 6. Discover same-origin sub-assets
    let html_sub_assets = scan_html_sub_assets(&html_text);

    // Queue of assets to fetch: (resolved_url, is_css, base_for_resolving)
    let mut pending_assets: Vec<(Url, bool)> = Vec::new();
    let mut seen_urls: HashSet<Url> = HashSet::new();

    for candidate in html_sub_assets {
        let resolved = match final_url.join(&candidate.raw_reference) {
            Ok(u) => u,
            Err(_) => continue,
        };

        if resolved.origin() != final_origin {
            warnings.push(ConvertWarning {
                code: "remote_asset_skipped".to_string(),
                message: format!("cross-origin asset '{}' skipped", candidate.raw_reference),
            });
            continue;
        }

        if seen_urls.insert(resolved.clone()) {
            pending_assets.push((resolved, candidate.is_css));
        }
    }

    if pending_assets.len() > MAX_SUB_ASSET_COUNT {
        return Err(ToolError::LimitExceeded);
    }

    // 7. Create workspace directories for offline storage
    let page_dir = workspace.root().join("page");
    let assets_dir = page_dir.join("assets");
    fs::create_dir_all(&assets_dir).map_err(|e| ToolError::OutputIo(e.to_string()))?;

    // Downloaded assets: map resolved URL -> local relative path
    let mut downloaded_map: BTreeMap<Url, (String, bool)> = BTreeMap::new();
    let mut downloaded_files: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    let mut asset_counter = 0;

    let mut current_idx = 0;
    while current_idx < pending_assets.len() {
        if cancel.is_cancelled() {
            return Err(ToolError::Interrupted);
        }
        let remaining = match total_deadline.checked_sub(start_time.elapsed()) {
            Some(r) if !r.is_zero() => r,
            _ => return Err(ToolError::LimitExceeded),
        };

        let (asset_url, is_css) = pending_assets[current_idx].clone();
        current_idx += 1;

        let req_timeout = remaining.min(client.policy().total_timeout);
        let read_timeout = remaining
            .min(client.policy().read_timeout)
            .min(PER_REQUEST_TIMEOUT);
        let asset_client = client.with_timeouts(req_timeout, read_timeout);

        let asset_result = asset_client.get_same_origin(asset_url.clone(), &final_origin);
        let (asset_resp, effective_asset_url) = match asset_result {
            Ok(pair) => pair,
            Err(NetError::CrossOriginRedirectRefused { to, .. }) => {
                warnings.push(ConvertWarning {
                    code: "remote_asset_skipped".to_string(),
                    message: format!("cross-origin asset redirect '{to}' skipped"),
                });
                continue;
            }
            Err(e) => return Err(map_net_error(e)),
        };

        if effective_asset_url.origin() != final_origin {
            warnings.push(ConvertWarning {
                code: "remote_asset_skipped".to_string(),
                message: format!("cross-origin asset redirect '{effective_asset_url}' skipped"),
            });
            continue;
        }

        if !asset_resp.is_success() {
            continue;
        }

        if asset_resp.body.len() > MAX_PER_ASSET_BYTES {
            return Err(ToolError::LimitExceeded);
        }

        cumulative_bytes = cumulative_bytes.saturating_add(asset_resp.body.len());
        if cumulative_bytes > MAX_CUMULATIVE_BYTES {
            return Err(ToolError::LimitExceeded);
        }

        let content_type = asset_resp.header("content-type").unwrap_or("");
        let ext = infer_extension(&effective_asset_url, content_type, is_css);
        let local_filename = format!("asset_{asset_counter}.{ext}");
        asset_counter += 1;

        let local_rel_path = format!("assets/{local_filename}");
        let local_disk_path = assets_dir.join(&local_filename);

        downloaded_map.insert(asset_url, (local_rel_path.clone(), is_css));
        downloaded_files.push((local_disk_path, asset_resp.body.clone()));

        // If CSS, scan for nested imports and url()
        if is_css {
            let css_text = String::from_utf8_lossy(&asset_resp.body);
            let css_candidates = scan_css_sub_assets(&css_text);

            for c in css_candidates {
                let resolved_sub = match effective_asset_url.join(&c.raw_reference) {
                    Ok(u) => u,
                    Err(_) => continue,
                };

                if resolved_sub.origin() != final_origin {
                    warnings.push(ConvertWarning {
                        code: "remote_asset_skipped".to_string(),
                        message: format!("cross-origin asset '{}' skipped in CSS", c.raw_reference),
                    });
                    continue;
                }

                if seen_urls.insert(resolved_sub.clone()) {
                    if pending_assets.len() >= MAX_SUB_ASSET_COUNT {
                        return Err(ToolError::LimitExceeded);
                    }
                    pending_assets.push((resolved_sub, c.is_css));
                }
            }
        }
    }

    // 8. Rewrite HTML and CSS assets
    let mut html_replacements: Vec<(String, String)> = Vec::new();
    let mut css_replacements: Vec<(String, String)> = Vec::new();

    for (url, (local_path, _is_css)) in &downloaded_map {
        let full_url_str = url.to_string();
        let path_and_query = format!(
            "{}{}",
            url.path(),
            url.query().map(|q| format!("?{q}")).unwrap_or_default()
        );
        let path_only = url.path().to_string();
        let file_name = url
            .path_segments()
            .and_then(|mut segs| segs.next_back())
            .unwrap_or("");
        let sibling_name = local_path.strip_prefix("assets/").unwrap_or(local_path);

        html_replacements.push((full_url_str.clone(), local_path.clone()));
        if !path_and_query.is_empty() && path_and_query != "/" {
            html_replacements.push((path_and_query.clone(), local_path.clone()));
        }
        if !path_only.is_empty() && path_only != "/" {
            html_replacements.push((path_only.clone(), local_path.clone()));
        }
        if !file_name.is_empty() {
            html_replacements.push((file_name.to_owned(), local_path.clone()));
        }

        css_replacements.push((full_url_str, sibling_name.to_owned()));
        if !path_and_query.is_empty() && path_and_query != "/" {
            css_replacements.push((path_and_query, sibling_name.to_owned()));
        }
        if !path_only.is_empty() && path_only != "/" {
            css_replacements.push((path_only, sibling_name.to_owned()));
        }
        if !file_name.is_empty() {
            css_replacements.push((file_name.to_owned(), sibling_name.to_owned()));
        }
    }

    // Sort replacements by match pattern length descending for determinism and prefix safety
    html_replacements.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
    css_replacements.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));

    // Write downloaded files to disk
    for (path, body) in downloaded_files {
        let is_css_file = path.extension().is_some_and(|e| e == "css");
        if is_css_file {
            let mut css_content = String::from_utf8_lossy(&body).to_string();
            for (from, to) in &css_replacements {
                css_content = css_content.replace(&format!("\"{from}\""), &format!("\"{to}\""));
                css_content = css_content.replace(&format!("'{from}'"), &format!("'{to}'"));
                css_content = css_content.replace(&format!("({from})"), &format!("({to})"));
            }
            fs::write(&path, css_content.as_bytes())
                .map_err(|e| ToolError::OutputIo(e.to_string()))?;
        } else {
            fs::write(&path, &body).map_err(|e| ToolError::OutputIo(e.to_string()))?;
        }
    }

    // Rewrite HTML
    let mut rewritten_html = html_text;
    for (from, to) in html_replacements {
        rewritten_html = rewritten_html.replace(&format!("\"{from}\""), &format!("\"{to}\""));
        rewritten_html = rewritten_html.replace(&format!("'{from}'"), &format!("'{to}'"));
        rewritten_html = rewritten_html.replace(&format!("={from}>"), &format!("={to}>"));
        rewritten_html = rewritten_html.replace(&format!("={from} "), &format!("={to} "));
    }

    let doc_html_path = page_dir.join("document.html");
    fs::write(&doc_html_path, rewritten_html.as_bytes())
        .map_err(|e| ToolError::OutputIo(e.to_string()))?;

    // 9. Run offline HTML -> IR -> Typst pipeline
    let base_dir = cap_std::fs::Dir::open_ambient_dir(&page_dir, ambient_authority())
        .map_err(|e| ToolError::OutputIo(e.to_string()))?;
    let asset_base = AssetBase::confined(base_dir);

    let read_result = crate::convert::read_document_to_ir(&doc_html_path, Format::Html, limits)
        .map_err(|e| ToolError::EngineFailure(e.to_string()))?;

    warnings.extend(read_result.warnings);

    let mut document = read_result.document;
    if let Some(ref title) = params.title {
        document.meta.title = Some(title.clone());
    }

    let asset_warnings = crate::assets::resolve_in(&mut document, &asset_base, limits);
    warnings.extend(asset_warnings.iter().map(safe_warning));

    let options = ariad_core::writer::typst::TypstOptions {
        title_fallback: params.title.clone(),
        ..Default::default()
    };
    let timestamp = clock.clock.epoch_secs;

    let rendered = crate::typst_render::render(&document, &options, timestamp)
        .map_err(|e| ToolError::EngineFailure(e.to_string()))?;

    for w in &rendered.writer_warnings {
        warnings.push(safe_warning(w));
    }

    let output_pdf_path = workspace.output_dir().join("document.pdf");
    fs::write(&output_pdf_path, &rendered.bytes).map_err(|e| ToolError::OutputIo(e.to_string()))?;

    Ok(RawToolExecution {
        artifacts: vec![(output_pdf_path, "pdf".to_string())],
        warnings,
        metrics: std::collections::BTreeMap::new(),
        inline_json: None,
    })
}

fn map_net_error(err: NetError) -> ToolError {
    match err {
        NetError::Timeout | NetError::ResponseTooLarge { .. } => ToolError::LimitExceeded,
        other => ToolError::EngineFailure(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
        sync::{Arc, Mutex},
    };

    use super::*;
    use crate::net::{HttpResponse, MockResolver, MockTransport};

    const TINY_PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 2,
        0, 0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 0, 0,
        3, 1, 1, 0, 201, 254, 146, 239, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    fn dummy_clock() -> HostClock {
        crate::tools::inprocess::capture_clock().expect("capture clock")
    }

    #[test]
    fn web_to_pdf_renders_html_and_same_origin_assets_to_pdf() {
        let resolver = Arc::new(MockResolver::new());
        let public_ip = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)), 443);
        resolver.insert("example.com", vec![public_ip]);

        let transport = Arc::new(MockTransport::new(move |req, _| {
            let path = req.url.path();
            if path == "/test.html" {
                let html = r#"<!DOCTYPE html>
<html>
<head>
  <title>Sample Web Page</title>
  <link rel="stylesheet" href="style.css">
</head>
<body>
  <h1>Sample Web Page</h1>
  <p>Rendered via Typst offline route.</p>
  <img src="cat.png" alt="A tiny cat image">
</body>
</html>"#;
                Ok(HttpResponse {
                    status: 200,
                    headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
                    body: html.as_bytes().to_vec(),
                })
            } else if path == "/style.css" {
                let css = "@import \"sub.css\";\nbody { background: url('bg.png'); }";
                Ok(HttpResponse {
                    status: 200,
                    headers: vec![("content-type".into(), "text/css".into())],
                    body: css.as_bytes().to_vec(),
                })
            } else if path == "/sub.css" {
                let sub_css = "h1 { color: red; }";
                Ok(HttpResponse {
                    status: 200,
                    headers: vec![("content-type".into(), "text/css".into())],
                    body: sub_css.as_bytes().to_vec(),
                })
            } else if path == "/cat.png" || path == "/bg.png" {
                Ok(HttpResponse {
                    status: 200,
                    headers: vec![("content-type".into(), "image/png".into())],
                    body: TINY_PNG.to_vec(),
                })
            } else {
                Ok(HttpResponse {
                    status: 404,
                    headers: vec![],
                    body: b"Not found".to_vec(),
                })
            }
        }));

        let client = Client::builder()
            .resolver(resolver)
            .transport(transport)
            .https_only(true)
            .build()
            .unwrap();

        let workspace = Workspace::new().unwrap();
        let spec = ariad_core::tools::find("web.to_pdf").unwrap();
        let params = serde_json::json!({
            "url": "https://example.com/test.html",
            "title": "My Offline Web PDF"
        });

        let cancel = CancellationToken::new();
        let scrubber = SecretScrubber::new();
        let raw_exec = execute_with_client(
            &client,
            spec,
            &params,
            &workspace,
            &Limits::local(),
            &scrubber,
            &cancel,
            &mut |_| {},
            dummy_clock(),
        )
        .expect("execution must succeed");

        assert_eq!(raw_exec.artifacts.len(), 1);
        let (path, format) = &raw_exec.artifacts[0];
        assert_eq!(format, "pdf");
        let bytes = fs::read(path).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(bytes.len() > 1000);

        let pdf_doc = lopdf::Document::load_mem(&bytes).expect("load rendered pdf");
        let text = pdf_doc.extract_text(&[1]).unwrap_or_default();
        assert!(
            text.contains("Sample Web Page"),
            "PDF missing heading text: {text}"
        );
        assert!(
            text.contains("Rendered via Typst offline route"),
            "PDF missing body text: {text}"
        );

        let has_image = pdf_doc
            .objects
            .values()
            .filter_map(|obj| obj.as_stream().ok())
            .any(|stream| {
                stream
                    .dict
                    .get(b"Subtype")
                    .ok()
                    .and_then(|o| o.as_name().ok())
                    == Some(b"Image".as_slice())
            });
        assert!(
            has_image,
            "rendered PDF must contain embedded Image XObject stream"
        );
    }

    #[test]
    fn web_to_pdf_rejects_private_loopback_metadata_and_mapped_ips() {
        let resolver = Arc::new(MockResolver::new());
        resolver.insert(
            "loopback.example",
            vec![SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                443,
            )],
        );
        resolver.insert(
            "private.example",
            vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 443)],
        );
        resolver.insert(
            "metadata.example",
            vec![SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)),
                443,
            )],
        );
        resolver.insert(
            "cgnat.example",
            vec![SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1)),
                443,
            )],
        );
        resolver.insert(
            "ipv6-loopback.example",
            vec![SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 443)],
        );
        resolver.insert(
            "mapped.example",
            vec![SocketAddr::new(
                IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0x7f00, 0x0001)),
                443,
            )],
        );
        resolver.insert(
            "nat64.example",
            vec![SocketAddr::new(
                IpAddr::V6(Ipv6Addr::new(0x0064, 0xff9b, 0, 0, 0, 0, 0x7f00, 0x0001)),
                443,
            )],
        );

        let transport = Arc::new(MockTransport::fixed(HttpResponse {
            status: 200,
            headers: vec![("content-type".into(), "text/html".into())],
            body: b"<html><body>Hi</body></html>".to_vec(),
        }));

        let client = Client::builder()
            .resolver(resolver)
            .transport(transport)
            .https_only(true)
            .allow_loopback(false)
            .build()
            .unwrap();

        let workspace = Workspace::new().unwrap();
        let spec = ariad_core::tools::find("web.to_pdf").unwrap();
        let scrubber = SecretScrubber::new();
        let cancel = CancellationToken::new();

        let targets = [
            "https://loopback.example/",
            "https://private.example/",
            "https://metadata.example/",
            "https://cgnat.example/",
            "https://ipv6-loopback.example/",
            "https://mapped.example/",
            "https://nat64.example/",
            "http://example.com/", // plain http must fail
        ];

        for target in targets {
            let params = serde_json::json!({ "url": target });
            let result = execute_with_client(
                &client,
                spec,
                &params,
                &workspace,
                &Limits::local(),
                &scrubber,
                &cancel,
                &mut |_| {},
                dummy_clock(),
            );
            assert!(
                result.is_err(),
                "target '{target}' must be rejected by SSRF/scheme guard"
            );
        }
    }

    #[test]
    fn web_to_pdf_blocks_dns_rebinding_and_unsafe_redirects() {
        let resolver = Arc::new(MockResolver::new());
        let public_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)), 443);
        let private_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 443);

        resolver.insert("public.example", vec![public_addr]);
        resolver.insert("private-target.example", vec![private_addr]);
        resolver.insert("mixed.example", vec![public_addr, private_addr]);

        let transport = Arc::new(MockTransport::new(move |req, _| match req.url.host_str() {
            Some("public.example") if req.url.path() == "/redirect-to-private" => {
                Ok(HttpResponse {
                    status: 302,
                    headers: vec![("location".into(), "https://private-target.example/".into())],
                    body: vec![],
                })
            }
            Some("public.example") if req.url.path() == "/redirect-to-http" => Ok(HttpResponse {
                status: 302,
                headers: vec![("location".into(), "http://public.example/plain".into())],
                body: vec![],
            }),
            Some("public.example") if req.url.path() == "/redirect-loop" => Ok(HttpResponse {
                status: 302,
                headers: vec![(
                    "location".into(),
                    "https://public.example/redirect-loop".into(),
                )],
                body: vec![],
            }),
            _ => Ok(HttpResponse {
                status: 200,
                headers: vec![("content-type".into(), "text/html".into())],
                body: b"<html><body>OK</body></html>".to_vec(),
            }),
        }));

        let client = Client::builder()
            .resolver(resolver)
            .transport(transport)
            .https_only(true)
            .allow_loopback(false)
            .build()
            .unwrap();

        let workspace = Workspace::new().unwrap();
        let spec = ariad_core::tools::find("web.to_pdf").unwrap();
        let scrubber = SecretScrubber::new();
        let cancel = CancellationToken::new();

        // 1. Redirect to private IP refused
        let res_priv = execute_with_client(
            &client,
            spec,
            &serde_json::json!({ "url": "https://public.example/redirect-to-private" }),
            &workspace,
            &Limits::local(),
            &scrubber,
            &cancel,
            &mut |_| {},
            dummy_clock(),
        );
        assert!(
            res_priv.is_err(),
            "redirect to private host must be refused"
        );

        // 2. Redirect to plain HTTP refused
        let res_http = execute_with_client(
            &client,
            spec,
            &serde_json::json!({ "url": "https://public.example/redirect-to-http" }),
            &workspace,
            &Limits::local(),
            &scrubber,
            &cancel,
            &mut |_| {},
            dummy_clock(),
        );
        assert!(
            res_http.is_err(),
            "redirect to http downgrade must be refused"
        );

        // 3. Redirect loop (> 5) refused
        let res_loop = execute_with_client(
            &client,
            spec,
            &serde_json::json!({ "url": "https://public.example/redirect-loop" }),
            &workspace,
            &Limits::local(),
            &scrubber,
            &cancel,
            &mut |_| {},
            dummy_clock(),
        );
        assert!(res_loop.is_err(), ">5 redirects must be refused");

        // 4. Mixed public and private DNS response rejected immediately
        let res_mixed = execute_with_client(
            &client,
            spec,
            &serde_json::json!({ "url": "https://mixed.example/" }),
            &workspace,
            &Limits::local(),
            &scrubber,
            &cancel,
            &mut |_| {},
            dummy_clock(),
        );
        assert!(
            res_mixed.is_err(),
            "DNS containing private IP must be refused"
        );
    }

    #[test]
    fn web_to_pdf_skips_cross_origin_assets_and_warns() {
        let resolver = Arc::new(MockResolver::new());
        let public_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)), 443);
        resolver.insert("main.example", vec![public_addr]);
        resolver.insert("cdn.other", vec![public_addr]);

        let requested_urls = Arc::new(Mutex::new(Vec::new()));
        let requested_urls_clone = Arc::clone(&requested_urls);

        let transport = Arc::new(MockTransport::new(move |req, _| {
            requested_urls_clone
                .lock()
                .unwrap()
                .push(req.url.to_string());
            if req.url.host_str() == Some("main.example") {
                let html = r#"<!DOCTYPE html>
<html>
<head>
  <link rel="stylesheet" href="https://cdn.other/style.css">
  <script src="https://main.example/app.js"></script>
</head>
<body>
  <p>Hello Cross Origin</p>
  <img src="https://cdn.other/tracker.png" alt="tracker">
</body>
</html>"#;
                Ok(HttpResponse {
                    status: 200,
                    headers: vec![("content-type".into(), "text/html".into())],
                    body: html.as_bytes().to_vec(),
                })
            } else {
                Ok(HttpResponse {
                    status: 200,
                    headers: vec![("content-type".into(), "text/plain".into())],
                    body: b"should not be called".to_vec(),
                })
            }
        }));

        let client = Client::builder()
            .resolver(resolver)
            .transport(transport)
            .https_only(true)
            .build()
            .unwrap();

        let workspace = Workspace::new().unwrap();
        let spec = ariad_core::tools::find("web.to_pdf").unwrap();
        let scrubber = SecretScrubber::new();
        let cancel = CancellationToken::new();

        let outcome = execute_with_client(
            &client,
            spec,
            &serde_json::json!({ "url": "https://main.example/index.html" }),
            &workspace,
            &Limits::local(),
            &scrubber,
            &cancel,
            &mut |_| {},
            dummy_clock(),
        )
        .expect("execution must succeed");

        let requested = requested_urls.lock().unwrap();
        for r in requested.iter() {
            assert!(
                !r.contains("cdn.other"),
                "cross-origin URL '{r}' was requested from transport!"
            );
        }

        assert!(
            outcome
                .warnings
                .iter()
                .any(|w| w.code == "remote_asset_skipped"),
            "must emit remote_asset_skipped"
        );
        assert!(
            outcome
                .warnings
                .iter()
                .any(|w| w.code == "js_content_not_rendered"),
            "must emit js_content_not_rendered"
        );
    }

    #[test]
    fn web_to_pdf_enforces_content_type_and_size_and_count_caps() {
        let resolver = Arc::new(MockResolver::new());
        let public_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)), 443);
        resolver.insert("caps.example", vec![public_addr]);

        let transport = Arc::new(MockTransport::new(move |req, _| match req.url.path() {
            "/pdf-type" => Ok(HttpResponse {
                status: 200,
                headers: vec![("content-type".into(), "application/pdf".into())],
                body: b"%PDF-1.4...".to_vec(),
            }),
            "/huge-html" => Ok(HttpResponse {
                status: 200,
                headers: vec![("content-type".into(), "text/html".into())],
                body: vec![b'a'; MAX_TOP_LEVEL_HTML_BYTES + 1],
            }),
            "/too-many-assets" => {
                let mut html = String::from("<!DOCTYPE html><html><body>");
                for i in 0..55 {
                    html.push_str(&format!("<img src=\"img_{i}.png\">\n"));
                }
                html.push_str("</body></html>");
                Ok(HttpResponse {
                    status: 200,
                    headers: vec![("content-type".into(), "text/html".into())],
                    body: html.into_bytes(),
                })
            }
            "/huge-cumulative" => {
                let html = "<!DOCTYPE html><html><body><img src=\"large.png\"></body></html>";
                Ok(HttpResponse {
                    status: 200,
                    headers: vec![("content-type".into(), "text/html".into())],
                    body: html.as_bytes().to_vec(),
                })
            }
            "/large.png" => Ok(HttpResponse {
                status: 200,
                headers: vec![("content-type".into(), "image/png".into())],
                body: vec![0u8; MAX_CUMULATIVE_BYTES + 1],
            }),
            _ => Ok(HttpResponse {
                status: 200,
                headers: vec![("content-type".into(), "image/png".into())],
                body: TINY_PNG.to_vec(),
            }),
        }));

        let client = Client::builder()
            .resolver(resolver)
            .transport(transport)
            .https_only(true)
            .max_response_bytes(MAX_CUMULATIVE_BYTES + 1024)
            .build()
            .unwrap();

        let workspace = Workspace::new().unwrap();
        let spec = ariad_core::tools::find("web.to_pdf").unwrap();
        let scrubber = SecretScrubber::new();
        let cancel = CancellationToken::new();

        // 1. Non-HTML content-type fails
        let res_content_type = execute_with_client(
            &client,
            spec,
            &serde_json::json!({ "url": "https://caps.example/pdf-type" }),
            &workspace,
            &Limits::local(),
            &scrubber,
            &cancel,
            &mut |_| {},
            dummy_clock(),
        );
        assert!(res_content_type.is_err(), "non-html content type must fail");

        // 2. HTML > 10 MiB fails
        let res_huge_html = execute_with_client(
            &client,
            spec,
            &serde_json::json!({ "url": "https://caps.example/huge-html" }),
            &workspace,
            &Limits::local(),
            &scrubber,
            &cancel,
            &mut |_| {},
            dummy_clock(),
        );
        assert!(res_huge_html.is_err(), "HTML > 10 MiB must fail");

        // 3. > 50 assets fails cleanly
        let res_too_many = execute_with_client(
            &client,
            spec,
            &serde_json::json!({ "url": "https://caps.example/too-many-assets" }),
            &workspace,
            &Limits::local(),
            &scrubber,
            &cancel,
            &mut |_| {},
            dummy_clock(),
        );
        assert!(res_too_many.is_err(), ">50 assets must fail");

        // 4. Cumulative bytes > 64 MiB fails cleanly
        let res_huge_cumul = execute_with_client(
            &client,
            spec,
            &serde_json::json!({ "url": "https://caps.example/huge-cumulative" }),
            &workspace,
            &Limits::local(),
            &scrubber,
            &cancel,
            &mut |_| {},
            dummy_clock(),
        );
        assert!(
            res_huge_cumul.is_err(),
            "cumulative bytes > 64 MiB must fail"
        );
    }

    #[test]
    fn web_to_pdf_sub_asset_cross_origin_redirect_is_blocked_before_request() {
        let resolver = Arc::new(MockResolver::new());
        let public_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)), 443);
        resolver.insert("example.com", vec![public_addr]);
        resolver.insert("cdn.other", vec![public_addr]);

        let transport = Arc::new(MockTransport::new(move |req, _| {
            let host = req.url.host_str().unwrap_or("");
            if host == "cdn.other" {
                panic!(
                    "MockTransport was invoked for cross-origin redirect target: {}",
                    req.url
                );
            }
            if host == "example.com" {
                if req.url.path() == "/index.html" {
                    let html = r#"<!DOCTYPE html>
<html>
<head><title>Safe Page</title></head>
<body>
  <h1>Safe Page</h1>
  <img src="r.png">
</body>
</html>"#;
                    Ok(HttpResponse {
                        status: 200,
                        headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
                        body: html.as_bytes().to_vec(),
                    })
                } else if req.url.path() == "/r.png" {
                    Ok(HttpResponse {
                        status: 302,
                        headers: vec![("location".into(), "https://cdn.other/pic.png".into())],
                        body: vec![],
                    })
                } else {
                    Ok(HttpResponse {
                        status: 404,
                        headers: vec![],
                        body: vec![],
                    })
                }
            } else {
                panic!("unexpected host requested: {host}");
            }
        }));

        let client = Client::builder()
            .resolver(resolver)
            .transport(transport)
            .https_only(true)
            .build()
            .unwrap();

        let workspace = Workspace::new().unwrap();
        let spec = ariad_core::tools::find("web.to_pdf").unwrap();
        let scrubber = SecretScrubber::new();
        let cancel = CancellationToken::new();

        let outcome = execute_with_client(
            &client,
            spec,
            &serde_json::json!({ "url": "https://example.com/index.html" }),
            &workspace,
            &Limits::local(),
            &scrubber,
            &cancel,
            &mut |_| {},
            dummy_clock(),
        )
        .expect("execution must succeed by skipping cross-origin redirected asset");

        let remote_asset_skipped = outcome
            .warnings
            .iter()
            .any(|w| w.code == "remote_asset_skipped");
        assert!(
            remote_asset_skipped,
            "must emit remote_asset_skipped warning, got: {:?}",
            outcome.warnings
        );
    }

    #[test]
    fn web_to_pdf_enforces_total_job_deadline_across_requests() {
        let resolver = Arc::new(MockResolver::new());
        let public_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)), 443);
        resolver.insert("timeout.example", vec![public_addr]);

        let transport = Arc::new(MockTransport::new(move |req, _| {
            if req.url.path() == "/index.html" {
                let html = r#"<!DOCTYPE html>
<html>
<head><title>Timeout Page</title></head>
<body>
  <h1>Timeout Test</h1>
  <img src="asset1.png">
  <img src="asset2.png">
</body>
</html>"#;
                Ok(HttpResponse {
                    status: 200,
                    headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
                    body: html.as_bytes().to_vec(),
                })
            } else if req.url.path() == "/asset1.png" {
                // Sleep past the total budget to trigger deadline expiry
                std::thread::sleep(Duration::from_millis(50));
                Ok(HttpResponse {
                    status: 200,
                    headers: vec![("content-type".into(), "image/png".into())],
                    body: TINY_PNG.to_vec(),
                })
            } else {
                Ok(HttpResponse {
                    status: 200,
                    headers: vec![("content-type".into(), "image/png".into())],
                    body: TINY_PNG.to_vec(),
                })
            }
        }));

        let client = Client::builder()
            .resolver(resolver)
            .transport(transport)
            .total_timeout(Duration::from_millis(30))
            .https_only(true)
            .build()
            .unwrap();

        let workspace = Workspace::new().unwrap();
        let spec = ariad_core::tools::find("web.to_pdf").unwrap();
        let scrubber = SecretScrubber::new();
        let cancel = CancellationToken::new();

        let result = execute_with_client_deadline(
            &client,
            Duration::from_millis(30),
            spec,
            &serde_json::json!({ "url": "https://timeout.example/index.html" }),
            &workspace,
            &Limits::local(),
            &scrubber,
            &cancel,
            &mut |_| {},
            dummy_clock(),
        );

        assert!(
            matches!(result, Err(ToolError::LimitExceeded)),
            "expected ToolError::LimitExceeded when total job deadline is exceeded across requests, got: {result:?}"
        );
    }
}

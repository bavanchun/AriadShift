//! Parameter types for the `web.to_pdf` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for `web.to_pdf`.
///
/// Converts a remote web page into a PDF document via host-side SSRF-safe fetching
/// followed by offline HTML -> IR -> Typst rendering.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WebToPdfParams {
    /// Remote web page URL to convert. Must be a valid HTTPS URL.
    pub url: String,
    /// Optional document title override. If omitted, the HTML title is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl WebToPdfParams {
    /// Validates the semantic constraints on `WebToPdfParams`.
    pub fn validate(&self) -> Result<(), String> {
        let trimmed_url = self.url.trim();
        if trimmed_url.is_empty() {
            return Err("url must not be empty".to_string());
        }

        let Some((scheme, rest)) = trimmed_url.split_once(':') else {
            return Err("url must contain a scheme (https://...)".to_string());
        };

        if !scheme.eq_ignore_ascii_case("https") {
            let sanitized = if let Some((before_query, _)) = trimmed_url.split_once('?') {
                before_query
            } else {
                trimmed_url
            };
            return Err(format!("url '{sanitized}' must use https scheme"));
        }

        if !rest.starts_with("//") {
            return Err("url must start with https://".to_string());
        }

        let authority_and_path = &rest[2..];
        let host_end = authority_and_path
            .find(['/', '?', '#'])
            .unwrap_or(authority_and_path.len());
        let host_port_user = &authority_and_path[..host_end];

        let host_port = if let Some((_, hp)) = host_port_user.split_once('@') {
            hp
        } else {
            host_port_user
        };

        if host_port.is_empty() {
            return Err("url host must not be empty".to_string());
        }

        let host = if host_port.starts_with('[') {
            let Some(closing) = host_port.find(']') else {
                return Err("invalid IPv6 host in url".to_string());
            };
            &host_port[1..closing]
        } else if let Some((h, _)) = host_port.split_once(':') {
            h
        } else {
            host_port
        };

        if host.is_empty() {
            return Err("url host must not be empty".to_string());
        }

        if let Some(ref title) = self.title
            && title.trim().is_empty()
        {
            return Err("title must not be empty if provided".to_string());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_params() {
        let p = WebToPdfParams {
            url: "https://example.com/page".to_string(),
            title: Some("Example Page".to_string()),
        };
        assert!(p.validate().is_ok());

        let json = serde_json::to_string(&p).unwrap();
        let de: WebToPdfParams = serde_json::from_str(&json).unwrap();
        assert_eq!(p, de);
    }

    #[test]
    fn test_rejects_non_https_url() {
        let p = WebToPdfParams {
            url: "http://example.com/page?query=secret".to_string(),
            title: None,
        };
        let err = p.validate().unwrap_err();
        assert!(err.contains("must use https scheme"));
        assert!(!err.contains("secret"));
    }

    #[test]
    fn test_rejects_empty_url() {
        let p = WebToPdfParams {
            url: "".to_string(),
            title: None,
        };
        assert!(p.validate().is_err());
    }

    #[test]
    fn test_rejects_empty_title_if_provided() {
        let p = WebToPdfParams {
            url: "https://example.com".to_string(),
            title: Some("   ".to_string()),
        };
        assert!(p.validate().is_err());
    }
}

//! Shared link URL validation policy.
//!
//! Restricts links across readers and writers to safe schemes (`http`, `https`, `mailto`)
//! and same-document fragment identifiers (`#`).

/// Checks whether the given URL uses an allowed link scheme or same-document fragment.
///
/// Returns `true` for:
/// - Same-document fragment identifiers starting with `#`
/// - Explicit schemes: `http`, `https`, and `mailto` (case-insensitive)
///
/// Returns `false` for:
/// - Leading or trailing whitespace
/// - Unknown schemes, dangerous schemes (`javascript:`, `data:`, `file:`)
/// - Ill-formed schemes or scheme-relative URLs
#[must_use]
pub fn allowed_link(url: &str) -> bool {
    if url.starts_with('#') {
        return true;
    }
    if url.trim() != url {
        return false;
    }
    let Some((scheme, _)) = url.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    if !chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        || !chars.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
    {
        return false;
    }
    scheme.eq_ignore_ascii_case("http")
        || scheme.eq_ignore_ascii_case("https")
        || scheme.eq_ignore_ascii_case("mailto")
}

/// Alias for [`allowed_link`].
#[must_use]
pub fn is_allowed_link(url: &str) -> bool {
    allowed_link(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_links_accept_http_https_mailto_and_anchors() {
        assert!(allowed_link("#section-1"));
        assert!(allowed_link("#"));
        assert!(allowed_link("http://example.com"));
        assert!(allowed_link("https://example.com/path?query=1#frag"));
        assert!(allowed_link("mailto:user@example.test"));
        assert!(allowed_link("HTTP://EXAMPLE.COM"));
        assert!(allowed_link("Https://Example.Com/"));
        assert!(allowed_link("MAILTO:INFO@EXAMPLE.COM"));
    }

    #[test]
    fn allowed_links_reject_dangerous_schemes_and_whitespace() {
        assert!(!allowed_link("javascript:alert(1)"));
        assert!(!allowed_link("data:text/html,<script>alert(1)</script>"));
        assert!(!allowed_link("file:///etc/passwd"));
        assert!(!allowed_link("vbscript:msgbox(1)"));
        assert!(!allowed_link(r"\\host\share"));
        assert!(!allowed_link(" https://example.com"));
        assert!(!allowed_link("https://example.com "));
        assert!(!allowed_link("relative/path/to/doc"));
        assert!(!allowed_link(""));
        assert!(!allowed_link(":"));
        assert!(!allowed_link("1http://example.com"));
    }
}

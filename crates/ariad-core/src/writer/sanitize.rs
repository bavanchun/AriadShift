//! Shared HTML sanitization for writers using Ammonia.

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

static ALLOWED_TAGS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "a",
        "abbr",
        "acronym",
        "area",
        "article",
        "aside",
        "b",
        "bdi",
        "bdo",
        "blockquote",
        "br",
        "caption",
        "cite",
        "code",
        "col",
        "colgroup",
        "data",
        "dd",
        "del",
        "details",
        "dfn",
        "div",
        "dl",
        "dt",
        "em",
        "figcaption",
        "figure",
        "footer",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "header",
        "hgroup",
        "hr",
        "i",
        "img",
        "ins",
        "kbd",
        "li",
        "main",
        "map",
        "mark",
        "nav",
        "ol",
        "p",
        "pre",
        "q",
        "rp",
        "rt",
        "rtc",
        "ruby",
        "s",
        "samp",
        "small",
        "span",
        "strike",
        "strong",
        "sub",
        "summary",
        "sup",
        "table",
        "tbody",
        "td",
        "tfoot",
        "th",
        "thead",
        "time",
        "tr",
        "u",
        "ul",
        "var",
        "wbr",
    ]
    .into_iter()
    .collect()
});

static GENERIC_ATTRS: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| ["title", "lang", "dir"].into_iter().collect());

static TAG_ATTRS: LazyLock<HashMap<&'static str, HashSet<&'static str>>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert("a", ["href", "hreflang"].into_iter().collect());
    m.insert(
        "img",
        ["src", "alt", "width", "height", "title"]
            .into_iter()
            .collect(),
    );
    m.insert("ol", ["start", "reversed", "type"].into_iter().collect());
    m.insert("col", ["span", "width"].into_iter().collect());
    m.insert("colgroup", ["span", "width"].into_iter().collect());
    m.insert("time", ["datetime"].into_iter().collect());
    m.insert("details", ["open"].into_iter().collect());
    m.insert("data", ["value"].into_iter().collect());
    m.insert("meter", ["value"].into_iter().collect());
    m.insert("blockquote", ["cite"].into_iter().collect());
    m.insert("q", ["cite"].into_iter().collect());
    m.insert("ins", ["cite"].into_iter().collect());
    m.insert("del", ["cite"].into_iter().collect());
    m.insert(
        "th",
        ["colspan", "rowspan", "headers", "scope"]
            .into_iter()
            .collect(),
    );
    m.insert(
        "td",
        ["colspan", "rowspan", "headers", "scope"]
            .into_iter()
            .collect(),
    );
    m
});

struct RelativeUrlEvaluator;

impl<'a> ammonia::UrlRelativeEvaluate<'a> for RelativeUrlEvaluator {
    fn evaluate<'b>(&self, url: &'b str) -> Option<Cow<'b, str>> {
        if crate::links::allowed_link(url) {
            Some(Cow::Borrowed(url))
        } else {
            None
        }
    }
}

/// Sanitizes raw HTML using ammonia with the shared project policy.
#[must_use]
pub fn sanitize_raw_html(raw: &str) -> String {
    let schemes: HashSet<&str> = ["http", "https", "mailto"].into_iter().collect();

    let mut builder = ammonia::Builder::new();
    builder
        .tags(ALLOWED_TAGS.clone())
        .generic_attributes(GENERIC_ATTRS.clone())
        .tag_attributes(TAG_ATTRS.clone())
        .url_schemes(schemes)
        .url_relative(ammonia::UrlRelative::Custom(Box::new(RelativeUrlEvaluator)))
        .link_rel(Some("noopener noreferrer"));
    builder.clean(raw).to_string()
}

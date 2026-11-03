//! Parameter types for the `pdf.viewer_prefs` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// Catalog `/PageLayout` setting for `pdf.viewer_prefs`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ViewerPageLayout {
    /// Display one page at a time (`/SinglePage`).
    #[serde(alias = "SinglePage", alias = "single_page")]
    SinglePage,
    /// Display the pages in one continuous column (`/OneColumn`).
    #[serde(alias = "OneColumn", alias = "one_column")]
    OneColumn,
    /// Display the pages in two columns, with odd-numbered pages on the left (`/TwoColumnLeft`).
    #[serde(alias = "TwoColumnLeft", alias = "two_column_left")]
    TwoColumnLeft,
    /// Display the pages in two columns, with odd-numbered pages on the right (`/TwoColumnRight`).
    #[serde(alias = "TwoColumnRight", alias = "two_column_right")]
    TwoColumnRight,
    /// Display two pages at a time, with odd-numbered pages on the left (`/TwoPageLeft`).
    #[serde(alias = "TwoPageLeft", alias = "two_page_left")]
    TwoPageLeft,
    /// Display two pages at a time, with odd-numbered pages on the right (`/TwoPageRight`).
    #[serde(alias = "TwoPageRight", alias = "two_page_right")]
    TwoPageRight,
}

/// Catalog `/PageMode` setting for `pdf.viewer_prefs`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ViewerPageMode {
    /// Neither document outline nor thumbnail images visible (`/UseNone`).
    #[serde(alias = "UseNone", alias = "use_none", alias = "none")]
    UseNone,
    /// Document outline (bookmarks) visible (`/UseOutlines`).
    #[serde(alias = "UseOutlines", alias = "use_outlines", alias = "outlines")]
    UseOutlines,
    /// Thumbnail images visible (`/UseThumbs`).
    #[serde(alias = "UseThumbs", alias = "use_thumbs", alias = "thumbs")]
    UseThumbs,
    /// Full-screen mode (`/FullScreen`).
    #[serde(alias = "FullScreen", alias = "full_screen")]
    FullScreen,
    /// Optional content group (layers) panel visible (`/UseOC`).
    #[serde(alias = "UseOC", alias = "use_oc", alias = "layers")]
    UseOc,
    /// Attachments panel visible (`/UseAttachments`).
    #[serde(
        alias = "UseAttachments",
        alias = "use_attachments",
        alias = "attachments"
    )]
    UseAttachments,
}

/// Parameters for `pdf.viewer_prefs`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewerPrefsParams {
    /// Initial page layout (`/PageLayout` on the Catalog).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_layout: Option<ViewerPageLayout>,
    /// Initial navigation panel mode (`/PageMode` on the Catalog).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_mode: Option<ViewerPageMode>,
    /// Whether to hide the viewer application's toolbars (`/ViewerPreferences /HideToolbar`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hide_toolbar: Option<bool>,
    /// Whether to resize the document window to fit the first page (`/ViewerPreferences /FitWindow`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit_window: Option<bool>,
    /// Whether the window title bar should display the document `/Title` (`/ViewerPreferences /DisplayDocTitle`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_doc_title: Option<bool>,
    /// 1-based page number to open initially (`/OpenAction` destination, `>= 1`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_open_page"
    )]
    #[schemars(range(min = 1))]
    pub open_page: Option<u32>,
}

fn deserialize_optional_open_page<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<u32>::deserialize(deserializer)?;
    if matches!(opt, Some(0)) {
        return Err(serde::de::Error::custom("open_page must be >= 1"));
    }
    Ok(opt)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn viewer_prefs_params_defaults_unknown_field_and_valid_invalid_json() {
        let defaults: ViewerPrefsParams = serde_json::from_value(json!({})).unwrap();
        assert_eq!(defaults, ViewerPrefsParams::default());

        let valid: ViewerPrefsParams = serde_json::from_value(json!({
            "page_layout": "two-column-left",
            "page_mode": "use-outlines",
            "hide_toolbar": true,
            "fit_window": false,
            "display_doc_title": true,
            "open_page": 2
        }))
        .unwrap();
        assert_eq!(valid.page_layout, Some(ViewerPageLayout::TwoColumnLeft));
        assert_eq!(valid.page_mode, Some(ViewerPageMode::UseOutlines));
        assert_eq!(valid.display_doc_title, Some(true));
        assert_eq!(valid.open_page, Some(2));

        assert!(serde_json::from_value::<ViewerPrefsParams>(json!({"open_page": 0})).is_err());
        assert!(serde_json::from_value::<ViewerPrefsParams>(json!({"unknown": 1})).is_err());
    }
}

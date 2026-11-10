//! Specifications for PDF page furniture and colour layers.
use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{
        BackgroundParams, BatesParams, HeaderFooterParams, InvertColorsParams, PageNumbersParams,
        TocPageParams, schema_for_params,
    },
};
/// Page furniture runs in the pure-Rust PDF engine.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    macro_rules! spec {
        ($id:literal,$summary:literal,$params:ty) => {
            ToolSpec {
                id: $id,
                version: "1",
                group: ToolGroup::Overlay,
                summary: $summary,
                inputs: InputArity::One,
                input_formats: &["pdf"],
                output_format: "pdf",
                output_formats: &[],
                params_schema: schema_for_params::<$params>,
                tier: Tier::Free,
                runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local, ToolRuntime::Cloud],
                engine: "ariad-pdf",
                secret_params: &[],
            }
        };
    }
    vec![
        spec!("pdf.page_numbers", "Add page numbers.", PageNumbersParams),
        spec!(
            "pdf.header_footer",
            "Add headers and footers.",
            HeaderFooterParams
        ),
        spec!("pdf.bates", "Add a Bates sequence.", BatesParams),
        spec!(
            "pdf.toc_page",
            "Insert linked contents pages from the outline.",
            TocPageParams
        ),
        spec!("pdf.background", "Add a page background.", BackgroundParams),
        spec!(
            "pdf.invert_colors",
            "Invert page content colours.",
            InvertColorsParams
        ),
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn furniture_contracts_and_defaults() {
        let specs = specs();
        assert_eq!(specs.len(), 6);
        for spec in &specs {
            assert_eq!(spec.group, ToolGroup::Overlay);
            assert_eq!(spec.inputs, InputArity::One);
            assert_eq!(spec.engine, "ariad-pdf");
            assert_eq!(spec.output_format, "pdf");
            assert!(spec.runtimes.contains(&ToolRuntime::Wasm));
            assert!(spec.runtimes.contains(&ToolRuntime::Local));
        }
        let numbers: PageNumbersParams = serde_json::from_value(json!({})).unwrap();
        assert!(numbers.validate().is_ok());
        assert_eq!(numbers.format, "{n}");
        assert_eq!(numbers.pages.as_str(), "all");
        let bates: BatesParams = serde_json::from_value(json!({})).unwrap();
        assert!(bates.validate().is_ok());
        assert_eq!(bates.digits, 6);
        let header: HeaderFooterParams = serde_json::from_value(json!({})).unwrap();
        assert!(header.validate().is_err());
        let toc: TocPageParams = serde_json::from_value(json!({})).unwrap();
        assert!(toc.validate().is_ok());
        assert_eq!(toc.max_depth, 3);
        let _: BackgroundParams = serde_json::from_value(json!({})).unwrap();
        let _: InvertColorsParams = serde_json::from_value(json!({})).unwrap();
    }
}

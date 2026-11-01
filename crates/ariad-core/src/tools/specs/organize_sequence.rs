//! Tool specifications for sequence-manipulation tools in the `organize` group
//! (`pdf.reorder`, `pdf.alternate_mix`, `pdf.insert_blank`).

use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{
        alternate_mix::AlternateMixParams, insert_blank::InsertBlankParams, reorder::ReorderParams,
        schema_for_params,
    },
};

/// Returns the tool specifications registered by this module.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            id: "pdf.reorder",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Reorder PDF pages using an explicit page range sequence or a named preset (reverse, duplex-interleave, odd-then-even).",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<ReorderParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.alternate_mix",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Interleave pages alternately from two PDF documents, optionally reversing the second document.",
            inputs: InputArity::many(2, 2),
            input_formats: &["pdf"],
            output_format: "pdf",
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<AlternateMixParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.insert_blank",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Insert blank pages of matching or specified paper size after selected PDF pages.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<InsertBlankParams>,
            secret_params: &[],
        },
    ]
}

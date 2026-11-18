//! Typed views of the host tool `metrics` object for Swift.
//!
//! `edit.export_document` reports `metrics.fidelity` (a `fidelity/2` measurement) and
//! `metrics.change_list` (block ids added, removed and modified). The host keeps them in an
//! untyped metrics map and leaves `inline_json` empty, so the bindings decode them here once.

use std::collections::{BTreeMap, HashMap};

use ariad_core::{fidelity, tools::params::BlockChangeList};
use serde_json::Value;

/// Counts behind a fidelity component: how many items were expected, found and matched.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FidelityCoverage {
    pub reference: u64,
    pub hypothesis: u64,
    pub matched: u64,
    pub observed: u64,
}

/// Precision, recall and F1 for one structural block kind.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FidelityStructureScore {
    pub counts: FidelityCoverage,
    pub precision: Option<f64>,
    pub recall: Option<f64>,
    pub f1: Option<f64>,
}

/// Measured document fidelity (`fidelity/2`). Every score is `None` when it is not applicable.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FidelityReport {
    pub metric_version: String,
    /// `measured` or `unavailable`.
    pub status: String,
    /// `runtime_readback` or `independent_reference`.
    pub basis: String,
    pub score: Option<f64>,
    pub text_cer: Option<f64>,
    pub structure_f1: Option<f64>,
    pub style_accuracy: Option<f64>,
    pub layout_iou: Option<f64>,
    pub structure_by_kind: HashMap<String, FidelityStructureScore>,
    pub coverage: HashMap<String, FidelityCoverage>,
    pub unavailable_reasons: HashMap<String, String>,
}

/// Block ids that differ between the baseline and the exported document.
#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct ChangeList {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub modified: Vec<String>,
}

fn coverage(c: &fidelity::Coverage) -> FidelityCoverage {
    FidelityCoverage {
        reference: c.reference as u64,
        hypothesis: c.hypothesis as u64,
        matched: c.matched as u64,
        observed: c.observed as u64,
    }
}

/// The snake_case wire name of a unit enum value.
fn wire_name(value: serde_json::Result<Value>) -> String {
    match value {
        Ok(Value::String(s)) => s,
        _ => String::new(),
    }
}

impl From<fidelity::FidelityReport> for FidelityReport {
    fn from(r: fidelity::FidelityReport) -> Self {
        Self {
            metric_version: r.metric_version,
            status: wire_name(serde_json::to_value(r.status)),
            basis: wire_name(serde_json::to_value(r.basis)),
            score: r.score,
            text_cer: r.text_cer,
            structure_f1: r.structure_f1,
            style_accuracy: r.style_accuracy,
            layout_iou: r.layout_iou,
            structure_by_kind: r
                .structure_by_kind
                .into_iter()
                .map(|(kind, s)| {
                    (
                        kind,
                        FidelityStructureScore {
                            counts: coverage(&s.counts),
                            precision: s.precision,
                            recall: s.recall,
                            f1: s.f1,
                        },
                    )
                })
                .collect(),
            coverage: r
                .coverage
                .into_iter()
                .map(|(name, c)| (name, coverage(&c)))
                .collect(),
            unavailable_reasons: r.unavailable_reasons.into_iter().collect(),
        }
    }
}

impl From<BlockChangeList> for ChangeList {
    fn from(c: BlockChangeList) -> Self {
        Self {
            added: c.added,
            removed: c.removed,
            modified: c.modified,
        }
    }
}

/// Decodes `metrics.fidelity` when present and well formed.
pub(crate) fn fidelity_from_metrics(metrics: &BTreeMap<String, Value>) -> Option<FidelityReport> {
    let value = metrics.get("fidelity")?;
    serde_json::from_value::<fidelity::FidelityReport>(value.clone())
        .ok()
        .map(Into::into)
}

/// Decodes `metrics.change_list` when present and well formed.
pub(crate) fn change_list_from_metrics(metrics: &BTreeMap<String, Value>) -> Option<ChangeList> {
    let value = metrics.get("change_list")?;
    serde_json::from_value::<BlockChangeList>(value.clone())
        .ok()
        .map(Into::into)
}

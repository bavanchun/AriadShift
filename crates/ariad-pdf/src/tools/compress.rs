//! PDF compression with bounded image decoding and deterministic serialization.

use ariad_core::tools::params::CompressParams;

use crate::{
    PdfDoc, PdfError, ToolCtx,
    optimize::{self, presets},
    tools::{InputDoc, InputSource, OutputDoc, ToolWarning},
    writer::{self, SaveOptions},
};

/// Compress one input, keeping its original bytes when no trial reduces its size.
pub fn run_inputs(
    inputs: &[InputDoc],
    params: &CompressParams,
    ctx: &ToolCtx,
) -> Result<(Vec<PdfDoc>, Vec<OutputDoc>), PdfError> {
    params
        .validate()
        .map_err(|error| PdfError::InvalidParams(error.to_string()))?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(
            "pdf.compress requires exactly one input".into(),
        ));
    }
    let input = &inputs[0];
    let original = input.open_for_mutation(ctx)?;
    let source = match &input.source {
        InputSource::Bytes(bytes) => bytes.clone(),
        InputSource::Doc(_) => {
            let mut copy = original.clone();
            writer::save(&mut copy, ctx)?
        }
    };
    let mut working = original.clone();
    let mut warnings: Vec<ToolWarning> = Vec::new();
    let input_bytes = source.len();
    optimize::structural(&mut working, &input.limits, ctx)?;
    if params.grayscale {
        let skipped = super::grayscale::convert(&mut working, &input.limits, ctx)?;
        if !skipped.is_empty() {
            warnings.push(warning("color_not_converted", skipped.join(", ")));
        }
    }
    let selected = presets::selected(params);
    let candidates = if selected.lossless {
        Vec::new()
    } else {
        optimize::images(&working, &input.limits, ctx)?
    };
    let mut profiles: Vec<_> = if params.target_bytes.is_some() && !selected.lossless {
        presets::LADDER
            .iter()
            .copied()
            .map(|p| presets::cap(p, params))
            .collect()
    } else {
        vec![selected]
    };
    profiles.dedup();
    if candidates.is_empty() {
        profiles.truncate(1);
    }
    let mut best: Option<(Vec<u8>, bool, presets::Profile)> = None;
    let mut evaluations = 0;
    for profile in profiles {
        ctx.check()?;
        evaluations += 1;
        let mut trial = working.clone();
        optimize::apply_images(&mut trial, &candidates, profile, ctx)?;
        let output = writer::save_with_options(
            &mut trial,
            ctx,
            &SaveOptions {
                object_streams: true,
                ..Default::default()
            },
        )?;
        let hit = params
            .target_bytes
            .is_none_or(|target| output.len() as u64 <= target);
        match &best {
            None => best = Some((output, hit, profile)),
            Some((_, false, _)) if hit => best = Some((output, hit, profile)),
            Some((current, false, _)) if !hit && output.len() < current.len() => {
                best = Some((output, hit, profile))
            }
            _ => {}
        }
        if hit {
            break;
        }
    }
    let (candidate, _, chosen) = best.ok_or(PdfError::Malformed)?;
    let bytes = if candidate.len() < source.len() {
        candidate
    } else {
        warnings.push(warning("no_size_gain", "original input returned".into()));
        source
    };
    if let Some(target) = params.target_bytes
        && bytes.len() as u64 > target
    {
        warnings.push(warning(
            "target_not_reached",
            format!("closest output is {} bytes", bytes.len()),
        ));
    }
    let output_bytes = bytes.len();
    let mut output = OutputDoc::from_bytes(String::new(), "pdf", bytes);
    output.coded_warnings = warnings;
    output
        .metrics
        .insert("input_bytes".into(), serde_json::json!(input_bytes));
    output
        .metrics
        .insert("output_bytes".into(), serde_json::json!(output_bytes));
    output.metrics.insert(
        "ratio".into(),
        serde_json::json!(output_bytes as f64 / input_bytes as f64),
    );
    output.metrics.insert(
        "target_bytes".into(),
        serde_json::json!(params.target_bytes),
    );
    output
        .metrics
        .insert("evaluations".into(), serde_json::json!(evaluations));
    output
        .metrics
        .insert("chosen_dpi".into(), serde_json::json!(chosen.dpi));
    output
        .metrics
        .insert("chosen_quality".into(), serde_json::json!(chosen.quality));
    Ok((vec![original], vec![output]))
}

fn warning(code: &str, message: String) -> ToolWarning {
    ToolWarning {
        code: code.into(),
        message,
    }
}

//! Format names and route planning for the API.

use ariad_core::{
    format::Format,
    planner::{self, Capabilities, Plan, PlanError, Profile},
};

use crate::packs::Pack;

/// Resolves a user-supplied format name: a registry id (`markdown`,
/// `docx`) or an extension (`md`, `.htm`). Internal interchange formats are
/// not accepted.
#[must_use]
pub fn resolve_format(name: &str) -> Option<Format> {
    let name = name.trim();
    let format = serde_json::from_value::<Format>(serde_json::Value::String(name.to_owned()))
        .ok()
        .or_else(|| Format::from_extension(name))?;
    (format != Format::PandocJson).then_some(format)
}

/// Detects an upload's format from its first bytes and file name.
#[must_use]
pub fn detect_format(prefix: &[u8], filename: &str) -> Option<Format> {
    let extension = std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str());
    ariad_core::format::classify(prefix, None, extension).filter(|f| *f != Format::PandocJson)
}

/// The `--to` argument the host converter accepts for a format.
#[must_use]
pub fn target_arg(format: Format) -> &'static str {
    format.extensions().first().copied().unwrap_or(format.id())
}

/// The file extension of an output.
#[must_use]
pub fn output_extension(format: Format) -> &'static str {
    target_arg(format)
}

/// Engines the deployment can run, given the packs that have workers.
#[must_use]
pub fn deployable_engines(caps: &Capabilities, packs: &[Pack]) -> Vec<String> {
    caps.registered_engines()
        .into_iter()
        .filter(|engine| packs.contains(&Pack::for_engine(engine)))
        .collect()
}

/// Plans a route over the deployable engines.
pub fn plan(from: Format, to: Format, profile: Profile, packs: &[Pack]) -> Result<Plan, String> {
    let caps = planner::embedded();
    let engines = deployable_engines(caps, packs);
    planner::plan(caps, from, to, profile, &engines).map_err(|err| match err {
        PlanError::NoRoute { reachable, .. } => {
            let mut targets: Vec<&str> = reachable
                .iter()
                .filter(|f| !matches!(f, Format::AriadIrJson | Format::PandocJson))
                .map(|f| f.id())
                .collect();
            targets.sort_unstable();
            targets.dedup();
            if targets.is_empty() {
                format!("no route from {}; no reachable targets", from.id())
            } else {
                format!(
                    "no route from {} to {}; reachable targets: {}",
                    from.id(),
                    to.id(),
                    targets.join(", ")
                )
            }
        }
        PlanError::SameFormat { .. } => {
            format!("source and target are both {}", from.id())
        }
        PlanError::InvalidCapabilities(err) => format!("invalid capabilities: {err}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_names_resolve_by_id_or_extension() {
        assert_eq!(resolve_format("markdown"), Some(Format::Markdown));
        assert_eq!(resolve_format("md"), Some(Format::Markdown));
        assert_eq!(resolve_format(".DOCX"), Some(Format::Docx));
        assert_eq!(resolve_format("pandoc+json"), None);
        assert_eq!(resolve_format("nope"), None);
        assert_eq!(target_arg(Format::Markdown), "md");
        assert_eq!(target_arg(Format::Docx), "docx");
    }

    #[test]
    fn markdown_to_docx_plans_on_the_core_pack() {
        let plan = plan(
            Format::Markdown,
            Format::Docx,
            Profile::Editable,
            &[Pack::Core],
        )
        .expect("route");
        assert_eq!(Pack::for_route(&plan.steps), Pack::Core);
        assert_eq!(plan.steps.first().map(|s| s.from), Some(Format::Markdown));
        assert_eq!(plan.steps.last().map(|s| s.to), Some(Format::Docx));
    }

    #[test]
    fn unreachable_targets_explain_alternatives() {
        let err = plan(
            Format::Markdown,
            Format::Png,
            Profile::Editable,
            &[Pack::Core],
        )
        .expect_err("no route");
        assert!(err.contains("no route from markdown"), "{err}");
    }

    #[test]
    fn detection_prefers_content_over_extension() {
        assert_eq!(
            detect_format(b"<!DOCTYPE html><p>x</p>", "notes.md"),
            Some(Format::Html)
        );
        assert_eq!(
            detect_format(b"# Title", "notes.md"),
            Some(Format::Markdown)
        );
        assert_eq!(detect_format(b"plain", "notes"), None);
    }
}

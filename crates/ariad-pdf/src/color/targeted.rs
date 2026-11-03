//! Paint-time colour mapping while preserving original colour-state operators.

use ariad_core::tools::params::ColorTarget;
use lopdf::{
    Dictionary, Document, Object,
    content::{Content, Operation},
};

use super::{ColorSpace, space};
use crate::{PdfError, ToolCtx};

/// Maximum decoded operators accepted from one page or form.
pub(crate) const MAX_OPERATIONS: usize = 1_000_000;
pub(crate) type Warnings = std::collections::BTreeMap<(&'static str, &'static str), usize>;
pub(crate) fn warn(warnings: &mut Warnings, operator: &'static str, reason: &'static str) {
    let count = warnings.entry((operator, reason)).or_default();
    *count = count.saturating_add(1);
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Color {
    Device {
        space: ColorSpace,
        components: Vec<f32>,
        name: Option<Vec<u8>>,
    },
    Unsupported,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct State {
    pub fill: Color,
    pub stroke: Color,
    pub text_mode: u8,
}
impl Default for State {
    fn default() -> Self {
        let black = Color::Device {
            space: ColorSpace::Gray,
            components: vec![0.0],
            name: None,
        };
        Self {
            fill: black.clone(),
            stroke: black,
            text_mode: 0,
        }
    }
}

fn operand_number(value: &Object) -> Option<f32> {
    match value {
        Object::Integer(v) => Some(*v as f32),
        Object::Real(v) => Some(*v),
        _ => None,
    }
}
fn components(operands: &[Object], space: ColorSpace) -> Option<Vec<f32>> {
    let values: Vec<f32> = operands.iter().map(operand_number).collect::<Option<_>>()?;
    (values.len() == space.count() && values.iter().all(|v| v.is_finite())).then_some(values)
}
fn default_components(space: ColorSpace) -> Vec<f32> {
    match space {
        ColorSpace::Gray => vec![0.0],
        ColorSpace::Rgb => vec![0.0; 3],
        ColorSpace::Cmyk => vec![0.0, 0.0, 0.0, 1.0],
    }
}
fn restore(color: &Color, stroke: bool) -> Vec<Operation> {
    match color {
        Color::Device {
            space: _,
            components,
            name: Some(name),
        } => vec![
            Operation::new(
                if stroke { "CS" } else { "cs" },
                vec![Object::Name(name.clone())],
            ),
            Operation::new(
                if stroke { "SC" } else { "sc" },
                components.iter().copied().map(Object::Real).collect(),
            ),
        ],
        Color::Device {
            space,
            components,
            name: None,
        } => vec![Operation::new(
            space.operator(stroke),
            components.iter().copied().map(Object::Real).collect(),
        )],
        Color::Unsupported => Vec::new(),
    }
}

/// Convert a supported device family to RGB using the same CMYK approximation
/// as the existing grayscale mapper.
pub(crate) fn rgb(space: ColorSpace, c: &[f32]) -> Option<[f32; 3]> {
    if c.len() != space.count() || c.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let c: Vec<_> = c.iter().map(|v| v.clamp(0.0, 1.0)).collect();
    Some(match space {
        ColorSpace::Gray => [c[0]; 3],
        ColorSpace::Rgb => [c[0], c[1], c[2]],
        ColorSpace::Cmyk => [
            (1.0 - c[0]) * (1.0 - c[3]),
            (1.0 - c[1]) * (1.0 - c[3]),
            (1.0 - c[2]) * (1.0 - c[3]),
        ],
    })
}

fn mapped<F>(
    state: &Color,
    stroke: bool,
    mapper: &F,
    warnings: &mut Warnings,
) -> Option<(Operation, Vec<Operation>)>
where
    F: Fn([f32; 3]) -> Option<[f32; 3]>,
{
    let Color::Device {
        space, components, ..
    } = state
    else {
        warn(
            warnings,
            if stroke { "stroke" } else { "fill" },
            "unsupported colour space",
        );
        return None;
    };
    let source = rgb(*space, components)?;
    let mapped = mapper(source)?;
    if mapped
        .iter()
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return None;
    }
    Some((
        Operation::new(
            if stroke { "RG" } else { "rg" },
            mapped.into_iter().map(Object::Real).collect(),
        ),
        restore(state, stroke),
    ))
}

/// Rewrite paint operations while leaving original colour operators and other
/// content intact. A callback receives each Form/Image invocation and its
/// inherited graphics state, so the document walker can clone resources safely.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rewrite<F, G>(
    doc: &Document,
    resources: &Dictionary,
    content: &Content,
    initial: &State,
    target: ColorTarget,
    max_depth: u16,
    ctx: &ToolCtx,
    mapper: &F,
    mut xobject: G,
) -> Result<(Content, Warnings), PdfError>
where
    F: Fn([f32; 3]) -> Option<[f32; 3]>,
    G: FnMut(&Operation, &State) -> Result<Operation, PdfError>,
{
    if content.operations.len() > MAX_OPERATIONS {
        return Err(PdfError::LimitExceeded(
            ariad_core::reader::pdf::ReadError::TooManyObjects {
                limit: MAX_OPERATIONS,
            },
        ));
    }
    let mut state = initial.clone();
    let mut stack = Vec::new();
    let mut output = Vec::with_capacity(content.operations.len());
    let mut warnings = Warnings::new();
    for (index, op) in content.operations.iter().enumerate() {
        if index.is_multiple_of(4096) {
            ctx.check()?;
        }
        if output.len() > MAX_OPERATIONS.saturating_sub(7) {
            return Err(PdfError::LimitExceeded(
                ariad_core::reader::pdf::ReadError::TooManyObjects {
                    limit: MAX_OPERATIONS,
                },
            ));
        }
        let name = op.operator.as_str();
        match name {
            "q" => stack.push(state.clone()),
            "Q" => state = stack.pop().ok_or(PdfError::Malformed)?,
            "Tr" => {
                let value = op
                    .operands
                    .first()
                    .and_then(|v| v.as_i64().ok())
                    .filter(|v| (0..=7).contains(v))
                    .ok_or(PdfError::Malformed)?;
                state.text_mode = value as u8;
            }
            "g" | "G" | "rg" | "RG" | "k" | "K" => {
                let stroke = matches!(name, "G" | "RG" | "K");
                let family = match name {
                    "g" | "G" => ColorSpace::Gray,
                    "rg" | "RG" => ColorSpace::Rgb,
                    _ => ColorSpace::Cmyk,
                };
                let value =
                    components(&op.operands, family).map_or(Color::Unsupported, |components| {
                        Color::Device {
                            space: family,
                            components,
                            name: None,
                        }
                    });
                if stroke {
                    state.stroke = value
                } else {
                    state.fill = value
                }
            }
            "cs" | "CS" => {
                let stroke = name == "CS";
                let color = op
                    .operands
                    .first()
                    .and_then(|v| v.as_name().ok())
                    .and_then(|n| {
                        space(doc, resources, n, max_depth).map(|space| Color::Device {
                            space,
                            components: default_components(space),
                            name: Some(n.to_vec()),
                        })
                    })
                    .unwrap_or(Color::Unsupported);
                if stroke {
                    state.stroke = color
                } else {
                    state.fill = color
                }
            }
            "sc" | "SC" | "scn" | "SCN" => {
                let stroke = matches!(name, "SC" | "SCN");
                let color = if stroke {
                    &mut state.stroke
                } else {
                    &mut state.fill
                };
                if let Color::Device {
                    space,
                    components: current,
                    ..
                } = color
                {
                    if let Some(values) = components(&op.operands, *space) {
                        *current = values;
                    } else {
                        *color = Color::Unsupported;
                    }
                }
            }
            _ => {}
        }
        if name == "Do" {
            output.push(xobject(op, &state)?);
            continue;
        }
        if name == "BI" || name == "sh" {
            if name == "BI" {
                warn(&mut warnings, "BI", "inline image unchanged");
            } else {
                warn(&mut warnings, "sh", "shading unchanged");
            }
        }
        let text = matches!(name, "Tj" | "TJ" | "'" | "\"");
        let fill = matches!(name, "f" | "F" | "f*" | "B" | "B*" | "b" | "b*");
        let stroke = matches!(name, "S" | "s" | "B" | "B*" | "b" | "b*");
        let paint_text = text && matches!(target, ColorTarget::Text | ColorTarget::All);
        let paint_vector =
            (fill || stroke) && matches!(target, ColorTarget::Vector | ColorTarget::All);
        let text_fill = paint_text && matches!(state.text_mode, 0 | 2 | 4 | 6);
        let text_stroke = paint_text && matches!(state.text_mode, 1 | 2 | 5 | 6);
        let fill_mapping = if text_fill || (paint_vector && fill) {
            mapped(&state.fill, false, mapper, &mut warnings)
        } else {
            None
        };
        let stroke_mapping = if text_stroke || (paint_vector && stroke) {
            mapped(&state.stroke, true, mapper, &mut warnings)
        } else {
            None
        };
        if let Some((set, _)) = &fill_mapping {
            output.push(set.clone());
        }
        if let Some((set, _)) = &stroke_mapping {
            output.push(set.clone());
        }
        output.push(op.clone());
        if let Some((_, restore)) = fill_mapping {
            output.extend(restore);
        }
        if let Some((_, restore)) = stroke_mapping {
            output.extend(restore);
        }
    }
    if !stack.is_empty() {
        return Err(PdfError::Malformed);
    }
    Ok((Content { operations: output }, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    fn run(input: &[u8], target: ColorTarget) -> Content {
        let doc = Document::with_version("1.7");
        let content = Content::decode(input).unwrap();
        rewrite(
            &doc,
            &dictionary! {},
            &content,
            &State::default(),
            target,
            16,
            &ToolCtx::new(),
            &|_| Some([1.0, 0.0, 0.0]),
            |op, _| Ok(op.clone()),
        )
        .unwrap()
        .0
    }

    #[test]
    fn text_mapping_restores_color_set_before_text() {
        let content = run(
            b"0.5 g BT /F1 12 Tf (text) Tj ET 0 0 5 5 re f",
            ColorTarget::Text,
        );
        let ops: Vec<_> = content
            .operations
            .iter()
            .map(|op| op.operator.as_str())
            .collect();
        assert_eq!(ops, ["g", "BT", "Tf", "rg", "Tj", "g", "ET", "re", "f"]);
        assert_eq!(content.operations[5].operands, vec![Object::Real(0.5)]);
    }

    #[test]
    fn vector_mapping_does_not_disturb_clipping_or_text() {
        let content = run(b"0 1 0 rg 0 0 5 5 re W f BT (a) Tj ET", ColorTarget::Vector);
        let ops: Vec<_> = content
            .operations
            .iter()
            .map(|op| op.operator.as_str())
            .collect();
        assert_eq!(ops, ["rg", "re", "W", "rg", "f", "rg", "BT", "Tj", "ET"]);
    }

    #[test]
    fn colors_follow_saved_graphics_state() {
        let content = run(
            b"0.5 g q 1 0 0 rg 0 0 1 1 re f Q 0 0 1 1 re f",
            ColorTarget::Vector,
        );
        let fills: Vec<_> = content
            .operations
            .iter()
            .filter(|op| op.operator == "g")
            .collect();
        assert_eq!(fills.len(), 2);
        assert_eq!(fills[1].operands, vec![Object::Real(0.5)]);
    }

    #[test]
    fn text_render_mode_selects_fill_and_stroke() {
        let content = run(b"0.5 g 0.2 G BT 2 Tr (x) Tj ET", ColorTarget::Text);
        let ops: Vec<_> = content
            .operations
            .iter()
            .map(|op| op.operator.as_str())
            .collect();
        assert_eq!(
            ops,
            ["g", "G", "BT", "Tr", "rg", "RG", "Tj", "g", "G", "ET"]
        );
    }

    #[test]
    fn cmyk_and_generic_components_are_restored_with_valid_arity() {
        let content = run(
            b"/DeviceCMYK cs 1 0 0 0 scn 0 0 1 1 re f 0 1 0 0 K 0 0 m 1 1 l S",
            ColorTarget::All,
        );
        let operations = &content.operations;
        let fill = operations.iter().position(|op| op.operator == "f").unwrap();
        assert_eq!(operations[fill + 1].operator, "cs");
        assert_eq!(operations[fill + 2].operator, "sc");
        assert_eq!(operations[fill + 2].operands.len(), 4);
        let stroke = operations.iter().position(|op| op.operator == "S").unwrap();
        assert_eq!(operations[stroke + 1].operator, "K");
        assert_eq!(operations[stroke + 1].operands.len(), 4);
    }

    #[test]
    fn icc_space_is_restored_and_pattern_remains_untouched() {
        let mut doc = Document::with_version("1.7");
        let profile = doc.add_object(lopdf::Stream::new(dictionary! { "N" => 3 }, vec![]));
        let resources = dictionary! { "ColorSpace" => dictionary! { "Profile" => vec![Object::Name(b"ICCBased".to_vec()), Object::Reference(profile)], "PatternSpace" => Object::Name(b"Pattern".to_vec()) } };
        let content = Content::decode(
            b"/Profile cs 0.1 0.2 0.3 sc 0 0 5 5 re f /PatternSpace cs /P1 scn 0 0 5 5 re f",
        )
        .unwrap();
        let (result, warnings) = rewrite(
            &doc,
            &resources,
            &content,
            &State::default(),
            ColorTarget::All,
            16,
            &ToolCtx::new(),
            &|_| Some([1.0, 0.0, 0.0]),
            |op, _| Ok(op.clone()),
        )
        .unwrap();
        let ops = &result.operations;
        let first_fill = ops.iter().position(|op| op.operator == "f").unwrap();
        assert_eq!(ops[first_fill + 1].operator, "cs");
        assert_eq!(
            ops[first_fill + 1].operands[0].as_name().unwrap(),
            b"Profile"
        );
        assert_eq!(ops[first_fill + 2].operator, "sc");
        assert_eq!(ops[first_fill + 2].operands.len(), 3);
        assert_eq!(
            warnings.get(&("fill", "unsupported colour space")),
            Some(&1)
        );
    }
}

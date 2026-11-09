//! Deliberately small inline CSS reader. Values become typed IR fields only.
use std::collections::BTreeSet;

use crate::ir::{CharacterStyle, Colour, FontRef, ParagraphAlignment, ParagraphStyle};

#[derive(Clone, Debug, Default)]
pub(super) struct InlineCss {
    pub paragraph: ParagraphStyle,
    pub character: CharacterStyle,
}

impl InlineCss {
    pub fn apply(&mut self, css: &str, inherited: &Self, ignored: &mut BTreeSet<String>) {
        let mut line_heights = Vec::new();
        for declaration in declarations(css).filter(|part| !part.trim().is_empty()) {
            let Some((property, raw)) = declaration.split_once(':') else {
                ignored.insert("declaration".into());
                continue;
            };
            let property = property.trim().to_ascii_lowercase();
            let raw = raw.trim();
            let value = raw.to_ascii_lowercase();
            // No opaque CSS is retained, and no resource-bearing syntax is interpreted.
            if raw.contains(['\\', '@', '{', '}', '<', '>', '!', ':'])
                || raw.chars().any(char::is_control)
            {
                ignored.insert(property);
                continue;
            }
            if property == "line-height" {
                line_heights.push((raw, value));
                continue;
            }
            if value == "inherit" {
                if !self.inherit(&property, inherited) {
                    ignored.insert(property.clone());
                }
                continue;
            }
            if !self.property(&property, raw, &value) {
                ignored.insert(property);
            }
        }
        for (raw, value) in line_heights {
            if value == "inherit" {
                self.paragraph.line_height = inherited.paragraph.line_height;
            } else if !self.property("line-height", raw, &value) {
                ignored.insert("line-height".into());
            }
        }
    }

    /// CSS box properties are local; only the inherited text/paragraph subset
    /// participates in the descendant's computed style.
    pub fn inherited(&self) -> Self {
        let mut style = self.clone();
        style.paragraph.space_before_pt = None;
        style.paragraph.space_after_pt = None;
        style.paragraph.indent_left_pt = None;
        style.paragraph.indent_right_pt = None;
        style.paragraph.background = None;
        style.character.highlight = None;
        style
    }

    pub fn has_box_style(&self) -> bool {
        self.paragraph.space_before_pt.is_some()
            || self.paragraph.space_after_pt.is_some()
            || self.paragraph.indent_left_pt.is_some()
            || self.paragraph.indent_right_pt.is_some()
            || self.paragraph.background.is_some()
            || self.character.highlight.is_some()
    }

    fn inherit(&mut self, property: &str, parent: &Self) -> bool {
        match property {
            "font-family" => {
                self.paragraph.font.clone_from(&parent.paragraph.font);
                self.character.font.clone_from(&parent.character.font);
            }
            "font-size" => {
                self.paragraph.size_pt = parent.paragraph.size_pt;
                self.character.size_pt = parent.character.size_pt;
            }
            "font-weight" => self.character.weight = parent.character.weight,
            "font-style" => self.character.italic = parent.character.italic,
            "text-decoration" | "text-decoration-line" => {
                self.character.underline = parent.character.underline;
                self.character.strike = parent.character.strike;
            }
            "color" => {
                self.paragraph.color.clone_from(&parent.paragraph.color);
                self.character.color.clone_from(&parent.character.color);
            }
            "background-color" => {
                self.paragraph
                    .background
                    .clone_from(&parent.paragraph.background);
                self.character
                    .highlight
                    .clone_from(&parent.character.highlight);
            }
            "line-height" => self.paragraph.line_height = parent.paragraph.line_height,
            "text-align" => self.paragraph.align = parent.paragraph.align,
            "margin-top" => self.paragraph.space_before_pt = parent.paragraph.space_before_pt,
            "margin-bottom" => self.paragraph.space_after_pt = parent.paragraph.space_after_pt,
            "text-indent" => self.paragraph.indent_first_pt = parent.paragraph.indent_first_pt,
            "padding-left" => self.paragraph.indent_left_pt = parent.paragraph.indent_left_pt,
            "padding-right" => self.paragraph.indent_right_pt = parent.paragraph.indent_right_pt,
            _ => return false,
        }
        true
    }

    fn property(&mut self, property: &str, raw: &str, value: &str) -> bool {
        match property {
            "font-family" => match font(raw) {
                Some(font) => {
                    self.paragraph.font = Some(font.clone());
                    self.character.font = Some(font);
                }
                None => return false,
            },
            "font-size" => match points(value).filter(|v| *v > 0.0) {
                Some(size) => {
                    self.paragraph.size_pt = Some(size);
                    self.character.size_pt = Some(size);
                }
                None => return false,
            },
            "font-weight" => {
                let weight = match value {
                    "normal" | "initial" => Some(400),
                    "bold" => Some(700),
                    _ => value.parse::<u16>().ok().filter(|v| (1..=1000).contains(v)),
                };
                let Some(weight) = weight else {
                    return false;
                };
                self.character.weight = Some(weight);
            }
            "font-style" => {
                self.character.italic = match value {
                    "normal" | "initial" => Some(false),
                    "italic" | "oblique" => Some(true),
                    _ => return false,
                }
            }
            "text-decoration" | "text-decoration-line" => {
                let tokens: Vec<_> = value.split_ascii_whitespace().collect();
                if tokens.is_empty()
                    || tokens
                        .iter()
                        .any(|t| !matches!(*t, "none" | "initial" | "underline" | "line-through"))
                    || (tokens.len() > 1 && tokens.iter().any(|t| matches!(*t, "none" | "initial")))
                {
                    return false;
                }
                self.character.underline = Some(tokens.contains(&"underline"));
                self.character.strike = Some(tokens.contains(&"line-through"));
            }
            "color" => match colour(value) {
                Some(colour) => {
                    self.paragraph.color = Some(colour.clone());
                    self.character.color = Some(colour);
                }
                None => return false,
            },
            "background-color" => match colour(value) {
                Some(colour) => {
                    self.paragraph.background = Some(colour.clone());
                    self.character.highlight = Some(colour);
                }
                None => return false,
            },
            "line-height" => {
                let Some(height) = line_height(value, self.character.size_pt) else {
                    return false;
                };
                self.paragraph.line_height = Some(height);
            }
            "text-align" => {
                self.paragraph.align = Some(match value {
                    "left" | "start" | "initial" => ParagraphAlignment::Left,
                    "right" | "end" => ParagraphAlignment::Right,
                    "center" => ParagraphAlignment::Center,
                    "justify" => ParagraphAlignment::Justify,
                    _ => return false,
                })
            }
            "margin-top" | "margin-bottom" | "text-indent" | "padding-left" | "padding-right" => {
                let Some(points) = points(value).filter(|v| property == "text-indent" || *v >= 0.0)
                else {
                    return false;
                };
                match property {
                    "margin-top" => self.paragraph.space_before_pt = Some(points),
                    "margin-bottom" => self.paragraph.space_after_pt = Some(points),
                    "text-indent" => self.paragraph.indent_first_pt = Some(points),
                    "padding-left" => self.paragraph.indent_left_pt = Some(points),
                    "padding-right" => self.paragraph.indent_right_pt = Some(points),
                    _ => unreachable!(),
                }
            }
            _ => return false,
        }
        true
    }
}

/// Delimiters inside strings or functions never introduce another declaration.
fn declarations(mut css: &str) -> impl Iterator<Item = &str> {
    std::iter::from_fn(move || {
        if css.is_empty() {
            return None;
        }
        let mut quote = None;
        let mut escaped = false;
        let mut parentheses = 0usize;
        for (index, byte) in css.bytes().enumerate() {
            if escaped {
                escaped = false;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                continue;
            }
            if let Some(end) = quote {
                if byte == end {
                    quote = None;
                }
            } else {
                match byte {
                    b'\'' | b'"' => quote = Some(byte),
                    b'(' => parentheses = parentheses.saturating_add(1),
                    b')' => parentheses = parentheses.saturating_sub(1),
                    b';' if parentheses == 0 => {
                        let part = &css[..index];
                        css = &css[index + 1..];
                        return Some(part);
                    }
                    _ => {}
                }
            }
        }
        Some(std::mem::take(&mut css))
    })
}

fn line_height(value: &str, size_pt: Option<f64>) -> Option<f64> {
    let height = if let Some(percent) = value.strip_suffix('%') {
        finite(percent).map(|v| v / 100.0)
    } else if value.ends_with("pt") || value.ends_with("px") {
        points(value).zip(size_pt).map(|(line, size)| line / size)
    } else {
        finite(value)
    };
    height.filter(|v| v.is_finite() && *v > 0.0)
}

fn finite(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}
fn points(value: &str) -> Option<f64> {
    if value == "0" {
        Some(0.0)
    } else if let Some(value) = value.strip_suffix("pt") {
        finite(value)
    } else {
        value
            .strip_suffix("px")
            .and_then(finite)
            .map(|v| v * 0.75)
            .filter(|v| v.is_finite())
    }
}
fn font(value: &str) -> Option<FontRef> {
    if value.len() > 1024 {
        return None;
    }
    let mut families = Vec::new();
    for name in value.split(',') {
        let name = name.trim();
        let name = if let Some(inner) = name.strip_prefix('"') {
            inner.strip_suffix('"')?
        } else if let Some(inner) = name.strip_prefix('\'') {
            inner.strip_suffix('\'')?
        } else {
            name
        };
        if name.is_empty()
            || name.len() > 256
            || !name
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        {
            return None;
        }
        families.push(name.to_owned());
        if families.len() > 16 {
            return None;
        }
    }
    if families.is_empty() {
        return None;
    }
    Some(FontRef {
        family: families.remove(0),
        fallback: families,
    })
}
fn colour(value: &str) -> Option<Colour> {
    let hex = match value {
        "black" => "#000000",
        "white" => "#ffffff",
        "red" => "#ff0000",
        "green" => "#008000",
        "blue" => "#0000ff",
        "yellow" => "#ffff00",
        "gray" | "grey" => "#808080",
        "silver" => "#c0c0c0",
        "maroon" => "#800000",
        "purple" => "#800080",
        "fuchsia" => "#ff00ff",
        "lime" => "#00ff00",
        "olive" => "#808000",
        "navy" => "#000080",
        "teal" => "#008080",
        "aqua" => "#00ffff",
        _ => value,
    };
    if hex.len() == 4
        && hex.starts_with('#')
        && hex.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
    {
        let mut expanded = String::from("#");
        for ch in hex[1..].chars() {
            expanded.push(ch);
            expanded.push(ch);
        }
        return Colour::new(expanded).ok();
    }
    if let Some(rgb) = value.strip_prefix("rgb(").and_then(|v| v.strip_suffix(')')) {
        let values: Option<Vec<u8>> = rgb.split(',').map(|v| v.trim().parse().ok()).collect();
        let values = values?;
        if let [r, g, b] = values.as_slice() {
            return Colour::new(format!("#{r:02x}{g:02x}{b:02x}")).ok();
        }
        return None;
    }
    Colour::new(hex).ok()
}

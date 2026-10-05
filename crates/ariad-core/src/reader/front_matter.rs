use saphyr_parser::{Event, EventReceiver, Parser};
use unicode_normalization::UnicodeNormalization;

use crate::{
    ir::Metadata,
    warning::{Warning, WarningCode},
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrontMatterOutput {
    pub metadata: Metadata,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, PartialEq)]
enum YamlNode {
    Scalar(String),
    Sequence(Vec<Self>),
    Mapping(Vec<(String, Self)>),
}

#[derive(Debug)]
enum Frame {
    Sequence(Vec<YamlNode>),
    Mapping {
        entries: Vec<(String, YamlNode)>,
        key: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ParseFailure {
    Invalid,
    AnchorOrAlias,
}

struct EventCollector<'input>(Vec<Event<'input>>);

impl<'input> EventReceiver<'input> for EventCollector<'input> {
    fn on_event(&mut self, event: Event<'input>) {
        self.0.push(event);
    }
}

/// Parses a front matter block, keeping malformed or unsafe metadata non-fatal.
pub fn parse_front_matter(yaml: &str, max_bytes: u32) -> FrontMatterOutput {
    if yaml.len() > max_bytes as usize {
        return invalid_output("front matter exceeds the configured byte limit");
    }

    let root = match parse_yaml_events(yaml) {
        Ok(root) => root,
        Err(ParseFailure::AnchorOrAlias) => {
            return FrontMatterOutput {
                metadata: Metadata::default(),
                warnings: vec![Warning::new(
                    WarningCode::FrontMatterAliasRejected,
                    "front matter anchors and aliases are not supported",
                )],
            };
        }
        Err(ParseFailure::Invalid) => {
            return invalid_output("front matter is invalid and was ignored");
        }
    };

    let YamlNode::Mapping(entries) = root else {
        return invalid_output("front matter must be a YAML mapping");
    };

    let mut output = FrontMatterOutput::default();
    for (key, value) in entries {
        let result = match key.as_str() {
            "title" => scalar(&value).map(|value| output.metadata.title = Some(nfc(value))),
            "author" | "authors" => {
                strings(&value).map(|values| output.metadata.authors = normalize(values))
            }
            "lang" => scalar(&value).map(|value| output.metadata.language = Some(nfc(value))),
            "date" => scalar(&value).map(|value| output.metadata.date = Some(nfc(value))),
            "subject" => scalar(&value).map(|value| output.metadata.subject = Some(nfc(value))),
            "keywords" => {
                strings(&value).map(|values| output.metadata.keywords = normalize(values))
            }
            _ => {
                output.warnings.push(Warning::new(
                    WarningCode::FrontMatterKeyIgnored,
                    format!("unknown front matter key `{key}` was ignored"),
                ));
                continue;
            }
        };

        if result.is_none() {
            return invalid_output(&format!(
                "front matter value for `{key}` must be a scalar or a list of scalars"
            ));
        }
    }

    output
}

fn parse_yaml_events(yaml: &str) -> Result<YamlNode, ParseFailure> {
    let mut parser = Parser::new_from_str(yaml);
    let mut collector = EventCollector(Vec::new());
    parser
        .load(&mut collector, true)
        .map_err(|_| ParseFailure::Invalid)?;

    let mut stack = Vec::new();
    let mut root = None;
    let mut document_count = 0;

    for event in collector.0 {
        match event {
            Event::DocumentStart(_) => document_count += 1,
            Event::Scalar(value, _, anchor, _) => {
                if anchor != 0 {
                    return Err(ParseFailure::AnchorOrAlias);
                }
                attach_node(&mut stack, &mut root, YamlNode::Scalar(value.into_owned()))?;
            }
            Event::SequenceStart(anchor, _) => {
                if anchor != 0 {
                    return Err(ParseFailure::AnchorOrAlias);
                }
                stack.push(Frame::Sequence(Vec::new()));
            }
            Event::SequenceEnd => {
                let Some(Frame::Sequence(values)) = stack.pop() else {
                    return Err(ParseFailure::Invalid);
                };
                attach_node(&mut stack, &mut root, YamlNode::Sequence(values))?;
            }
            Event::MappingStart(anchor, _) => {
                if anchor != 0 {
                    return Err(ParseFailure::AnchorOrAlias);
                }
                stack.push(Frame::Mapping {
                    entries: Vec::new(),
                    key: None,
                });
            }
            Event::MappingEnd => {
                let Some(Frame::Mapping { entries, key }) = stack.pop() else {
                    return Err(ParseFailure::Invalid);
                };
                if key.is_some() {
                    return Err(ParseFailure::Invalid);
                }
                attach_node(&mut stack, &mut root, YamlNode::Mapping(entries))?;
            }
            Event::Alias(_) => return Err(ParseFailure::AnchorOrAlias),
            Event::Nothing | Event::StreamStart | Event::StreamEnd | Event::DocumentEnd => {}
        }
    }

    if document_count != 1 || !stack.is_empty() {
        return Err(ParseFailure::Invalid);
    }
    root.ok_or(ParseFailure::Invalid)
}

fn attach_node(
    stack: &mut [Frame],
    root: &mut Option<YamlNode>,
    node: YamlNode,
) -> Result<(), ParseFailure> {
    match stack.last_mut() {
        Some(Frame::Sequence(values)) => values.push(node),
        Some(Frame::Mapping { entries, key }) => {
            if let Some(key) = key.take() {
                entries.push((key, node));
            } else if let YamlNode::Scalar(key_value) = node {
                *key = Some(key_value);
            } else {
                return Err(ParseFailure::Invalid);
            }
        }
        None if root.is_none() => *root = Some(node),
        None => return Err(ParseFailure::Invalid),
    }
    Ok(())
}

fn scalar(node: &YamlNode) -> Option<&str> {
    match node {
        YamlNode::Scalar(value) => Some(value),
        YamlNode::Sequence(_) | YamlNode::Mapping(_) => None,
    }
}

fn strings(node: &YamlNode) -> Option<Vec<String>> {
    match node {
        YamlNode::Scalar(value) => Some(vec![value.clone()]),
        YamlNode::Sequence(values) => values
            .iter()
            .map(|value| scalar(value).map(str::to_owned))
            .collect(),
        YamlNode::Mapping(_) => None,
    }
}

fn normalize(values: Vec<String>) -> Vec<String> {
    values.into_iter().map(|value| nfc(&value)).collect()
}

fn nfc(value: &str) -> String {
    value.nfc().collect()
}

fn invalid_output(message: &str) -> FrontMatterOutput {
    FrontMatterOutput {
        metadata: Metadata::default(),
        warnings: vec![Warning::new(WarningCode::FrontMatterInvalid, message)],
    }
}

#[cfg(test)]
mod tests {
    use super::parse_front_matter;
    use crate::warning::WarningCode;

    #[test]
    fn maps_known_fields_and_normalizes_metadata() {
        let output = parse_front_matter(
            "title: Cafe\u{301}\nauthor:\n  - Rene\u{301}\nlang: vi\ndate: 2026-10-06\nsubject: test\nkeywords: [one, two]\n",
            64 * 1024,
        );
        assert_eq!(output.metadata.title.as_deref(), Some("Café"));
        assert_eq!(output.metadata.authors, ["René"]);
        assert_eq!(output.metadata.language.as_deref(), Some("vi"));
        assert_eq!(output.metadata.date.as_deref(), Some("2026-10-06"));
        assert_eq!(output.metadata.subject.as_deref(), Some("test"));
        assert_eq!(output.metadata.keywords, ["one", "two"]);
        assert!(output.warnings.is_empty());
    }

    #[test]
    fn accepts_author_as_a_scalar_and_warns_on_unknown_keys() {
        let output = parse_front_matter("author: Jane Doe\ncustom: value\n", 1024);
        assert_eq!(output.metadata.authors, ["Jane Doe"]);
        assert_eq!(output.warnings.len(), 1);
        assert_eq!(output.warnings[0].code, WarningCode::FrontMatterKeyIgnored);
    }

    #[test]
    fn rejects_anchors_and_aliases_without_expanding_them() {
        for yaml in ["title: &x value\n", "title: &x value\nauthor: *x\n"] {
            let output = parse_front_matter(yaml, 1024);
            assert!(output.metadata.title.is_none());
            assert_eq!(output.warnings.len(), 1);
            assert_eq!(
                output.warnings[0].code,
                WarningCode::FrontMatterAliasRejected
            );
        }
    }

    #[test]
    fn invalid_yaml_and_wrong_value_shapes_are_ignored() {
        let invalid = parse_front_matter("title: [unterminated\n", 1024);
        assert_eq!(invalid.warnings[0].code, WarningCode::FrontMatterInvalid);
        assert!(invalid.metadata.title.is_none());

        let wrong_shape = parse_front_matter("title: [one, two]\n", 1024);
        assert_eq!(
            wrong_shape.warnings[0].code,
            WarningCode::FrontMatterInvalid
        );
        assert!(wrong_shape.metadata.title.is_none());
    }

    #[test]
    fn rejects_front_matter_over_the_cap_before_parsing() {
        let output = parse_front_matter("title: longer than cap\n", 5);
        assert_eq!(output.warnings[0].code, WarningCode::FrontMatterInvalid);
        assert!(output.metadata.title.is_none());
    }
}

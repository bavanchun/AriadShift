#![no_main]

use ariad_core::reader::front_matter::parse_front_matter;
use ariad_fuzz::nfc;
use libfuzzer_sys::fuzz_target;
use saphyr_parser::{Event, EventReceiver, Parser};

const MAX_INPUT_BYTES: usize = 128 * 1024;
const CAP: u32 = 64 * 1024;

struct EventCollector<'a>(Vec<Event<'a>>);

impl<'a> EventReceiver<'a> for EventCollector<'a> {
    fn on_event(&mut self, event: Event<'a>) {
        self.0.push(event);
    }
}

/// Independently parses YAML events to detect anchors or aliases.
fn contains_anchor_or_alias(yaml: &str) -> bool {
    let mut parser = Parser::new_from_str(yaml);
    let mut collector = EventCollector(Vec::new());
    if parser.load(&mut collector, true).is_err() {
        return false;
    }
    collector.0.into_iter().any(|event| match event {
        Event::Alias(_) => true,
        Event::Scalar(_, _, anchor, _)
        | Event::SequenceStart(anchor, _)
        | Event::MappingStart(anchor, _) => anchor != 0,
        _ => false,
    })
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }

    let yaml = String::from_utf8_lossy(data);
    let output = parse_front_matter(&yaml, CAP);

    // Invariant: No alias accepted.
    // Detect anchors/aliases independently from parse_front_matter,
    // and assert that non-default metadata implies no anchor or alias was present.
    let has_anchor_or_alias = contains_anchor_or_alias(&yaml);
    if output.metadata != Default::default() {
        assert!(
            !has_anchor_or_alias,
            "non-default metadata implies no anchor or alias present"
        );
    }

    // Invariant: all text stored in metadata must be NFC normalized
    if let Some(ref title) = output.metadata.title {
        assert_eq!(title, &nfc(title), "title must be NFC normalized");
    }
    for author in &output.metadata.authors {
        assert_eq!(author, &nfc(author), "author must be NFC normalized");
    }
    if let Some(ref lang) = output.metadata.language {
        assert_eq!(lang, &nfc(lang), "language must be NFC normalized");
    }
    if let Some(ref date) = output.metadata.date {
        assert_eq!(date, &nfc(date), "date must be NFC normalized");
    }
    if let Some(ref subject) = output.metadata.subject {
        assert_eq!(subject, &nfc(subject), "subject must be NFC normalized");
    }
    for kw in &output.metadata.keywords {
        assert_eq!(kw, &nfc(kw), "keyword must be NFC normalized");
    }
});

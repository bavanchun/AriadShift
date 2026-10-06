use std::io::Read;

use ariad_core::ir::Document;
use serde::Deserialize;

/// Reads an IR document without serde_json's default nesting limit.
pub fn read(reader: impl Read) -> Result<Document, serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_reader(reader);
    deserializer.disable_recursion_limit();
    let document = {
        let stacked = serde_stacker::Deserializer::new(&mut deserializer);
        Document::deserialize(stacked)?
    };
    deserializer.end()?;
    Ok(document)
}

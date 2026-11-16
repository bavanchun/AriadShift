//! Email document readers for EML and Outlook MSG formats.

pub mod eml;
pub mod msg;

pub use eml::read as read_eml;
pub use msg::read as read_msg;

/// Strips Unicode bidirectional control characters from filenames or metadata.
///
/// Removes ALM (U+061C), LRM/RLM (U+200E-U+200F), the embedding and override
/// controls (U+202A-U+202E) and the isolates (U+2066-U+2069), so a name such
/// as `evil\u{202E}fdp.exe` cannot display as a different extension.
pub(crate) fn strip_bidi_controls(s: &str) -> String {
    s.chars()
        .filter(|&c| {
            !matches!(
                c,
                '\u{061c}' | '\u{200e}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::strip_bidi_controls;

    #[test]
    fn strips_every_bidi_control_and_keeps_other_text() {
        let controls = [
            '\u{061c}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}',
            '\u{202e}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
        ];
        let input: String = controls
            .iter()
            .flat_map(|c| [*c, 'a'])
            .chain("Tiếng Việt \u{200d}".chars())
            .collect();
        assert_eq!(
            strip_bidi_controls(&input),
            "aaaaaaaaaaaaTiếng Việt \u{200d}"
        );
    }
}

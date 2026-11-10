//! Safe deterministic names for batch-filled PDFs.

use std::collections::HashSet;

/// Creates unique output names, including across case-insensitive filesystems.
#[derive(Default)]
pub struct BatchNames {
    used: HashSet<String>,
}

fn reserved(name: &str) -> bool {
    let base = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
}
fn clean(raw: &str) -> Option<String> {
    let filtered: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .collect();
    let trimmed = filtered.trim_matches('.').replace("..", "");
    let stem = if trimmed.to_ascii_lowercase().ends_with(".pdf") {
        &trimmed[..trimmed.len() - 4]
    } else {
        &trimmed
    };
    if stem.is_empty() || reserved(stem) {
        return None;
    }
    Some(stem.chars().take(96).collect())
}
impl BatchNames {
    /// Selects a sanitized record name or the numbered fallback, adding a suffix on collisions.
    pub fn allocate(
        &mut self,
        column_value: Option<&str>,
        input_stem: &str,
        index: usize,
        total: usize,
    ) -> String {
        let width = total.max(index).to_string().len().max(3);
        let fallback = || {
            let stem = crate::naming::sanitize_stem(input_stem);
            let stem = if reserved(&stem) { "document" } else { &stem };
            format!("{stem}.filled-{index:0width$}")
        };
        let base = column_value.and_then(clean).unwrap_or_else(fallback);
        let mut n = 1usize;
        loop {
            let suffix = if n == 1 {
                String::new()
            } else {
                format!("-{n}")
            };
            let available = 96usize.saturating_sub(suffix.len());
            let stem: String = base.chars().take(available).collect();
            let candidate = format!("{stem}{suffix}.pdf");
            if self.used.insert(candidate.to_ascii_lowercase()) {
                return candidate;
            }
            n += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_are_safe_bounded_and_unique() {
        let mut names = BatchNames::default();
        assert_eq!(names.allocate(Some("../x"), "input.pdf", 1, 3), "x.pdf");
        assert_eq!(names.allocate(Some("X"), "input.pdf", 2, 3), "X-2.pdf");
        assert_eq!(
            names.allocate(Some("CON"), "input.pdf", 3, 3),
            "input.filled-003.pdf"
        );
        assert!(
            names
                .allocate(Some(&"a".repeat(300)), "input.pdf", 4, 4)
                .len()
                <= 100
        );
    }
}

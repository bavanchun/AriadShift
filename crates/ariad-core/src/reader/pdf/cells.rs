//! What the cells of a whitespace-aligned candidate hold.
//!
//! Aligned columns alone do not make a table. Tables of contents, numbered clauses, verse with
//! line numbers, forms of labels and values, code with comments, bibliographies, indexes and
//! numbered equations all set short pieces of text in aligned columns. These checks read the text
//! of the cells and name those layouts, so that the detector can leave them as text.
//!
//! Each check looks at one column, or at the pattern of the columns, and only judges a column of
//! at least [`MIN_CELLS`] filled cells. The checks are deliberately blunt: a real table that looks
//! like one of these layouts, such as a ledger whose first column numbers the rows next to a
//! column of descriptions, is also left as text.

use super::lists;

/// Fewest filled cells for a column to be judged.
const MIN_CELLS: usize = 3;

/// Share of the cells of a column that must show a feature for the column to have it.
const SHARE: f64 = 0.6;

/// Share of the cells of a column that must be relation symbols.
const RELATION_SHARE: f64 = 0.8;

/// Share of all cells that end like the statements of a program.
const STATEMENT_SHARE: f64 = 0.4;

/// Words of letters that make a cell a phrase rather than a name or a label.
const PHRASE_WORDS: usize = 3;

/// Largest column count of a list set in repeated column groups.
const MAX_REPEATED_COLUMNS: usize = 8;

/// A layout that aligns text in columns without being a table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Layout {
    /// A column of list markers or of ascending numbers beside a column of text (a table of
    /// contents, numbered clauses, verse with line numbers), or a column of cells that each begin
    /// with a list marker, as the options of a quiz do.
    Enumeration,
    /// A column of phrases, as in running text, a bibliography or a list of definitions.
    Prose,
    /// Cells that end in a colon: the labels of a form.
    Labels,
    /// Comment markers or statements that end in a semicolon: program code.
    Code,
    /// Equation numbers or relation symbols.
    Equations,
    /// The same kinds of column twice over: a list set in two column groups, as an index is.
    Repeated,
}

/// The first layout of the columns that is not a table, if any. `columns` holds the text of the
/// filled cells of each column, top to bottom.
pub(super) fn recognise(columns: &[Vec<String>]) -> Option<Layout> {
    let judged = |column: &&Vec<String>| column.len() >= MIN_CELLS;
    if columns
        .iter()
        .filter(judged)
        .any(|column| share(column, is_phrase) >= SHARE)
    {
        return Some(Layout::Prose);
    }
    if columns
        .iter()
        .filter(judged)
        .any(|column| share(column, |text| text.ends_with(':')) >= SHARE)
    {
        return Some(Layout::Labels);
    }
    if columns
        .iter()
        .filter(judged)
        .any(|column| share(column, begins_with_marker) >= SHARE)
    {
        return Some(Layout::Enumeration);
    }
    if is_code(columns) {
        return Some(Layout::Code);
    }
    if is_equations(columns) {
        return Some(Layout::Equations);
    }
    if is_enumerated(columns) {
        return Some(Layout::Enumeration);
    }
    if is_repeated(columns) {
        return Some(Layout::Repeated);
    }
    None
}

fn share(column: &[String], test: impl Fn(&str) -> bool) -> f64 {
    column.iter().filter(|text| test(text)).count() as f64 / column.len().max(1) as f64
}

/// Words made of letters: tokens with at least two letters, so that "12", "(a)" and "T." do not
/// count.
fn letter_words(text: &str) -> usize {
    text.split_whitespace()
        .filter(|token| token.chars().filter(|ch| ch.is_alphabetic()).count() >= 2)
        .count()
}

fn is_phrase(text: &str) -> bool {
    letter_words(text) >= PHRASE_WORDS
}

/// Characters that appear in numbers, amounts and missing-value marks.
const NUMBER_MARKS: &[char] = &[
    '.', ',', '%', '(', ')', '-', '–', '—', '−', '+', '$', '€', '£', '¥', '/', ':', ' ',
];

/// Whether the text is a number: digits and number marks, a mark alone for a missing value, or a
/// short token that starts with digits such as `509k`.
fn is_number(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let digits = text.chars().any(|ch| ch.is_ascii_digit());
    if text
        .chars()
        .all(|ch| ch.is_ascii_digit() || NUMBER_MARKS.contains(&ch))
    {
        return true;
    }
    digits
        && text.chars().count() <= 8
        && !text.contains(' ')
        && text.starts_with(|ch: char| ch.is_ascii_digit())
}

/// Whether the column holds mostly something other than numbers.
fn is_textual(column: &[String]) -> bool {
    share(column, |text| !is_number(text)) >= SHARE
}

/// A list marker such as `1.`, `(a)` or a bullet. A lone dash or asterisk marks a missing value
/// in a table, not a list item.
fn is_marker(text: &str) -> bool {
    if text
        .chars()
        .all(|ch| matches!(ch, '-' | '–' | '—' | '*' | '+' | '.'))
    {
        return false;
    }
    lists::is_cell_marker_only(text)
}

/// Whether the text begins with a bullet or a numbered or lettered label and goes on: the item of
/// a list. A dash or asterisk does not count, because `+ 5` and `- 3` are amounts in a table.
fn begins_with_marker(text: &str) -> bool {
    lists::parse_cell(text)
        .is_some_and(|marker| marker.ordered || marker.strength == lists::MarkerStrength::Strong)
}

/// `1`, `12` or `2.3.1` (with an optional final dot) as its numbers.
fn numbering(text: &str) -> Option<Vec<u32>> {
    let parts: Vec<&str> = text.strip_suffix('.').unwrap_or(text).split('.').collect();
    if parts
        .iter()
        .any(|part| part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    parts.iter().map(|part| part.parse::<u32>().ok()).collect()
}

/// Whether `next` is the item that follows `previous`: the next sibling at some level, or the
/// first child.
fn follows(previous: &[u32], next: &[u32]) -> bool {
    if next.len() == previous.len() + 1 && next.starts_with(previous) && next[previous.len()] == 1 {
        return true;
    }
    (1..=previous.len()).any(|level| {
        next.len() == level
            && next[..level - 1] == previous[..level - 1]
            && next[level - 1] == previous[level - 1] + 1
    })
}

/// Whether the column numbers or marks the items of a list: mostly list markers, or numbers in
/// which most steps are to the following item.
fn is_enumeration(column: &[String]) -> bool {
    if column.len() < MIN_CELLS {
        return false;
    }
    if share(column, is_marker) >= 0.7 {
        return true;
    }
    let numbers: Vec<Option<Vec<u32>>> = column.iter().map(|text| numbering(text)).collect();
    if (numbers.iter().flatten().count() as f64) < 0.8 * column.len() as f64 {
        return false;
    }
    let steps: Vec<(&Vec<u32>, &Vec<u32>)> = numbers
        .windows(2)
        .filter_map(|pair| Some((pair[0].as_ref()?, pair[1].as_ref()?)))
        .collect();
    let following = steps
        .iter()
        .filter(|(previous, next)| follows(previous, next))
        .count();
    !steps.is_empty() && following as f64 >= 0.6 * steps.len() as f64
}

/// An enumeration in one of the first two columns, next to a column of text.
fn is_enumerated(columns: &[Vec<String>]) -> bool {
    columns.iter().enumerate().take(2).any(|(index, column)| {
        is_enumeration(column)
            && columns
                .iter()
                .enumerate()
                .any(|(other, text)| other != index && text.len() >= MIN_CELLS && is_textual(text))
    })
}

fn is_code(columns: &[Vec<String>]) -> bool {
    let comment =
        |text: &str| text.starts_with("//") || text.starts_with("/*") || text.starts_with("# ");
    if columns
        .iter()
        .any(|column| column.len() >= MIN_CELLS && share(column, comment) >= SHARE)
    {
        return true;
    }
    let all: Vec<String> = columns.iter().flatten().cloned().collect();
    all.len() >= 2 * MIN_CELLS
        && share(&all, |text| {
            text.ends_with(';') || text.ends_with('{') || text.ends_with('}')
        }) >= STATEMENT_SHARE
}

fn is_equations(columns: &[Vec<String>]) -> bool {
    let relation = |text: &str| {
        matches!(
            text,
            "=" | "<" | ">" | "≤" | "≥" | "≈" | "≠" | "≡" | "→" | ":=" | "∼"
        )
    };
    // Equation numbers: `(1)`, `(2)`, ... counting up.
    let tag = |text: &str| -> Option<u32> {
        let inner = text.strip_prefix('(')?.strip_suffix(')')?;
        (inner.len() <= 3).then(|| inner.parse().ok()).flatten()
    };
    columns
        .iter()
        .filter(|column| column.len() >= MIN_CELLS)
        .any(|column| {
            if share(column, relation) >= RELATION_SHARE {
                return true;
            }
            let tags: Vec<Option<u32>> = column.iter().map(|text| tag(text)).collect();
            if (tags.iter().flatten().count() as f64) < SHARE * column.len() as f64 {
                return false;
            }
            let steps: Vec<(u32, u32)> = tags
                .windows(2)
                .filter_map(|pair| Some((pair[0]?, pair[1]?)))
                .collect();
            let counting = steps.iter().filter(|(a, b)| *b == a + 1).count();
            !steps.is_empty() && counting as f64 >= SHARE * steps.len() as f64
        })
}

/// A list set in two (or three) groups of columns: the kinds of the first half of the columns
/// come again in the second half, and the columns are not all alike.
fn is_repeated(columns: &[Vec<String>]) -> bool {
    let count = columns.len();
    if !(4..=MAX_REPEATED_COLUMNS).contains(&count) || !count.is_multiple_of(2) {
        return false;
    }
    let mut kinds = Vec::with_capacity(count);
    for column in columns {
        if column.len() < MIN_CELLS {
            return false;
        }
        if share(column, is_number) >= SHARE {
            kinds.push(true);
        } else if is_textual(column) {
            kinds.push(false);
        } else {
            return false;
        }
    }
    let half = count / 2;
    kinds.iter().any(|&kind| kind) != kinds.iter().all(|&kind| kind)
        && (0..half).all(|index| kinds[index] == kinds[index + half])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(cells: &[&str]) -> Vec<String> {
        cells.iter().map(|cell| (*cell).to_owned()).collect()
    }

    #[test]
    fn a_table_of_contents_is_an_enumeration_beside_text() {
        let columns = [
            column(&["1", "1.1", "1.2", "2", "2.1", "2.2", "3", "4"]),
            column(&[
                "Introduction",
                "Background",
                "Scope",
                "Methods",
                "Sampling",
                "Tools",
                "Results",
                "End",
            ]),
            column(&["3", "4", "6", "9", "10", "13", "17", "25"]),
        ];
        assert_eq!(recognise(&columns), Some(Layout::Enumeration));
    }

    #[test]
    fn list_markers_in_the_first_columns_are_an_enumeration() {
        let columns = [
            column(&["1.", "2.", "3."]),
            column(&["(a)", "(a)", "(a)"]),
            column(&["Pay", "Ship", "Sign"]),
        ];
        assert_eq!(recognise(&columns), Some(Layout::Enumeration));
    }

    #[test]
    fn numbers_beside_numbers_are_not_an_enumeration() {
        let columns = [
            column(&["1", "2", "3", "4"]),
            column(&["0.5", "0.7", "0.9", "1.1"]),
            column(&["12", "15", "9", "20"]),
        ];
        assert_eq!(recognise(&columns), None);
    }

    #[test]
    fn decimals_that_do_not_count_up_are_not_numbering() {
        let columns = [
            column(&["0.5", "1.0", "1.5", "2.0"]),
            column(&["Low", "Mid", "High", "Top"]),
            column(&["3", "5", "8", "13"]),
        ];
        assert_eq!(recognise(&columns), None);
    }

    #[test]
    fn a_column_of_phrases_is_prose() {
        let columns = [
            column(&["ACL", "CNN", "OCR"]),
            column(&[
                "Association for Computational Linguistics",
                "Convolutional neural network",
                "Optical character recognition",
            ]),
            column(&["12", "14", "21"]),
        ];
        assert_eq!(recognise(&columns), Some(Layout::Prose));
    }

    #[test]
    fn names_of_two_words_and_dates_are_not_phrases() {
        let columns = [
            column(&["Height gauge", "Depth rod", "Dial gauge"]),
            column(&["12 May 2024", "14 May 2024", "15 May 2024"]),
            column(&["150", "200", "10"]),
        ];
        assert_eq!(recognise(&columns), None);
    }

    #[test]
    fn labels_that_end_in_a_colon_are_a_form() {
        let columns = [
            column(&["Name:", "Address:", "Phone:", "Account:"]),
            column(&["Jane", "Mill Lane", "555", "0048"]),
            column(&["Date:", "City:", "Email:", "Branch:"]),
            column(&["May", "Leeds", "j@x.org", "North"]),
        ];
        assert_eq!(recognise(&columns), Some(Layout::Labels));
    }

    #[test]
    fn comment_markers_and_statements_are_code() {
        let columns = [
            column(&["total", "count", "mean"]),
            column(&["= sum(items);", "= len(items);", "= total / count;"]),
            column(&["// all together", "// how many", "// the average"]),
        ];
        assert_eq!(recognise(&columns), Some(Layout::Code));
        let statements = [
            column(&["a = 1;", "b = 2;", "c = 3;"]),
            column(&["d = 4;", "e = 5;", "f = 6;"]),
        ];
        assert_eq!(recognise(&statements), Some(Layout::Code));
    }

    #[test]
    fn cells_that_begin_with_a_marker_are_list_items() {
        let options = [
            column(&["(a) 14", "(a) 13", "(a) 12"]),
            column(&["(b) 15", "(b) 14", "(b) 13"]),
            column(&["(c) 16", "(c) 15", "(c) 14"]),
        ];
        assert_eq!(recognise(&options), Some(Layout::Enumeration));
        // Amounts with a sign and a space are not markers.
        let signed = [
            column(&["+ 5", "- 3", "+ 12"]),
            column(&["Alpha", "Beta", "Gamma"]),
            column(&["1.5", "2.5", "3.5"]),
        ];
        assert_eq!(recognise(&signed), None);
    }

    #[test]
    fn counting_tags_and_relation_symbols_are_equations() {
        let tags = [
            column(&["x", "y", "z"]),
            column(&["a + b", "c - d", "2e"]),
            column(&["(1)", "(2)", "(3)"]),
        ];
        assert_eq!(recognise(&tags), Some(Layout::Equations));
        let relations = [
            column(&["x", "y", "z"]),
            column(&["=", "=", "="]),
            column(&["a + b", "c - d", "2e"]),
        ];
        assert_eq!(recognise(&relations), Some(Layout::Equations));
    }

    #[test]
    fn negative_amounts_in_parentheses_are_not_equation_tags() {
        let columns = [
            column(&["Rent", "Fuel", "Wages"]),
            column(&["12", "8", "40"]),
            column(&["(5)", "(12)", "(3)"]),
        ];
        assert_eq!(recognise(&columns), None);
    }

    #[test]
    fn text_and_numbers_twice_over_are_a_list_in_two_groups() {
        let columns = [
            column(&["Aperture", "Bokeh", "Contrast"]),
            column(&["12", "14", "15"]),
            column(&["Kernel", "Lens", "Macro"]),
            column(&["33", "35", "36"]),
        ];
        assert_eq!(recognise(&columns), Some(Layout::Repeated));
    }

    #[test]
    fn a_table_with_text_then_numbers_is_not_repeated() {
        let columns = [
            column(&["Tabula", "Camelot", "EDD"]),
            column(&["78.0", "80.0", "91.2"]),
            column(&["57.8", "66.0", "85.4"]),
            column(&["67.9", "73.0", "88.3"]),
        ];
        assert_eq!(recognise(&columns), None);
    }

    #[test]
    fn ledgers_with_long_descriptions_or_numbered_rows_are_left_as_text() {
        // Real tables, left as text on purpose: a text column of phrases beside amounts looks
        // like a table of contents, and a numbered first column beside names looks like a list.
        let ledger = [
            column(&[
                "Rent for the north office",
                "Wages for the spring term",
                "Fuel for the vans",
            ]),
            column(&["1,200", "8,400", "310"]),
            column(&["1,150", "8,100", "290"]),
        ];
        assert_eq!(recognise(&ledger), Some(Layout::Prose));
        let ranking = [
            column(&["1", "2", "3", "4"]),
            column(&["Alpha", "Beta", "Gamma", "Delta"]),
            column(&["9.1", "8.7", "8.2", "7.9"]),
        ];
        assert_eq!(recognise(&ranking), Some(Layout::Enumeration));
    }

    #[test]
    fn short_columns_are_not_judged() {
        let columns = [
            column(&["1.", "2."]),
            column(&["Pay", "Ship"]),
            column(&["a", "b"]),
        ];
        assert_eq!(recognise(&columns), None);
    }

    #[test]
    fn a_lone_dash_is_a_missing_value_not_a_list_marker() {
        let columns = [
            column(&["-", "-", "-", "-"]),
            column(&["Alpha", "Beta", "Gamma", "Delta"]),
            column(&["1.5", "2.5", "3.5", "4.5"]),
        ];
        assert_eq!(recognise(&columns), None);
    }

    #[test]
    fn bracketed_numbers_beside_text_are_a_list() {
        // The years are a column of numbers, so only the labels tell this from a table.
        let references = [
            column(&["[1]", "[2]", "[3]", "[4]"]),
            column(&["Smith", "Garcia", "Okafor", "Tanaka"]),
            column(&["2019", "2020", "2021", "2023"]),
            column(&[
                "Table detection",
                "Layout analysis",
                "Column finding",
                "Aligned text",
            ]),
        ];
        assert_eq!(recognise(&references), Some(Layout::Enumeration));
    }

    #[test]
    fn capital_letters_in_parentheses_label_list_items() {
        // A column of numbers is present, so only the labels tell this from a table.
        let parenthesised = [
            column(&["(A) 14", "(A) 13", "(A) 12"]),
            column(&["(B) 15", "(B) 14", "(B) 13"]),
            column(&["3", "4", "5"]),
        ];
        assert_eq!(recognise(&parenthesised), Some(Layout::Enumeration));
        let bare = [
            column(&["A) 14", "A) 13", "A) 12"]),
            column(&["B) 15", "B) 14", "B) 13"]),
            column(&["3", "4", "5"]),
        ];
        assert_eq!(recognise(&bare), Some(Layout::Enumeration));
    }

    #[test]
    fn initials_are_not_list_labels() {
        let authors = [
            column(&["A. Smith", "B. Jones", "K. Lee"]),
            column(&["12", "15", "9"]),
            column(&["3.5", "1.2", "7.7"]),
        ];
        assert_eq!(recognise(&authors), None);
    }

    #[test]
    fn numbering_follows_siblings_and_children() {
        assert!(follows(&[1], &[2]));
        assert!(follows(&[1], &[1, 1]));
        assert!(follows(&[1, 2], &[2]));
        assert!(follows(&[2, 2], &[3]));
        assert!(!follows(&[1], &[3]));
        assert!(!follows(&[0, 5], &[1, 0]));
        assert_eq!(numbering("2.3.1."), Some(vec![2, 3, 1]));
        assert_eq!(numbering("1,234"), None);
        assert_eq!(numbering("2024-05"), None);
    }
}

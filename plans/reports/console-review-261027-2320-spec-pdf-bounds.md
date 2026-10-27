# Fix spec: PDF reader bounds (I8)

Date: 2026-10-27 (Asia/Saigon). Ref: `origin/dev` at `0b118f6`. No commit since `e6bd29b` touches `crates/ariad-core`, so line numbers match the earlier reviews. Read-only: nothing was built, run or fuzzed. Every claim below comes from reading the code. Time bounds are estimates.

Inputs: master plan `console-review-261027-2045-master-fix-plan-final.md` §4 row I8; `console-review-261027-2015-dev-core.md` (DC) H7, M8–M10; `console-review-261027-2035-verify-dev-core.md` (V-DC), which overrides DC; `console-review-261027-2045-dev-core-repro-tests.md` (REPRO), whose PDF tests this spec reuses or relocates. `console-review-261027-2045-dev-fix-specs.md` has no PDF item.

## 1. Verdict

All five parts of I8 are still valid and ready to implement: H7, M8, M9, M10 and the module-doc correction. M8 also has a variant the reviews missed, which the same fix covers. Nothing is stale or blocked. No other group I item touches `reader/pdf/*`.

## 2. Summary table

| Item | Sev | Still valid | Branch | Files | Size | Worker grouping |
|---|---|---|---|---|---|---|
| I8 · H7: hyphen joining is quadratic | High | Yes | from `origin/dev`, e.g. `fix/pdf-reader-bounds` | `reader/pdf/assemble.rs` | S | W-PDF, commit 1 |
| I8 · M8: font array walks are not charged | Medium | Yes. There is also a within-font variant: `/W` entries that name one shared inner array | same | `reader/pdf/{objects,font}.rs`; tests in `interpreter.rs` | S | W-PDF, commit 2 |
| I8 · M9: parsed CMaps are not charged | Medium | Yes | same | `reader/pdf/{cmap,font,interpreter}.rs` | M | W-PDF, commit 3 |
| I8 · M10: no total cap on operand array elements | Medium | Yes | same | `reader/pdf/interpreter.rs` | S | W-PDF, commit 4 |
| I8 · module doc overstates the guarantee | Low (docs) | Yes | same | `reader/pdf/mod.rs:8-15` | S | W-PDF, commit 5 |
| I8 · end-to-end proving tests | n/a | n/a | same | new `crates/ariad-core/tests/pdf_reader_bounds.rs` | S | W-PDF, commit 6 |

Use one worker for all of I8, with sequential commits on one branch and one PR into `dev`. `interpreter.rs` takes changes from M8 tests, M9 and M10, and the new test file serves H7 and M8, so splitting the work would make two workers edit the same files. W-PDF's files (`crates/ariad-core/src/reader/pdf/*` plus the new test file) do not overlap with the I1, I5, I6 or I7 workers (`writer/*`, `reader/markdown.rs`, `reader/html*.rs`). Total size: M.

Placement rule from the master plan §4 I: do not add tests to `tests/pdf_reader.rs`. REPRO put two PDF tests there. This spec moves them into the new per-item file `tests/pdf_reader_bounds.rs` (§3.6).

## 3. Items

### 3.1 H7: hyphen joining reads the whole accumulated word on every line

**Current behaviour (origin/dev).**

- `join_lines` (`assemble.rs:647-653`) calls `append_line` for each line. `append_line` (`:628-644`) calls `line_join(&last.text, …)` on the last output span. `push_span` (`:655-666`) has already appended every earlier same-style line to that span.
- On `Join::Merge`, `append_line` pops the hyphen (`:636-638`), so a chain of `ab-` lines builds one ever-growing word.
- Inside `line_join` (`:593-625`), each of these costs O(k) in that word's length: `stem.rsplit(char::is_whitespace)` (`:598`), two `format!(…).to_lowercase()` calls (`:611`, `:614`) and `left.contains('-')` (`:615`).
- `Emitter::paragraph` calls `append_line` again at `:155` when a paragraph continues across a column or page. There the "previous" text is the whole open paragraph.
- I re-read layout (`layout.rs:843-960`). Identical `ab-` lines at one x, size and pitch pass every check in `continues()`, and no lines-per-block cap exists, which agrees with V-DC. For N lines the cost is about Σk ≈ N² characters per pass.

**The exact change.** The master plan says "join on the previous line's text and bound the left-word scan to 64 chars". The bound alone makes each join O(64 + first word of the next line). Every line is the `next` argument exactly once, so total work becomes linear, and the bound also covers the cross-block call at `:155`.

I recommend not switching to the previous line's text, for two reasons:

- It needs extra state on the `:155` path.
- It can pop the wrong character. `Merge` pops from `out.last()`, and that span can be a different-style whitespace-only span rather than the one holding the hyphen.

With the bound alone, joins are identical to today for left words of up to 64 characters.

```diff
--- a/crates/ariad-core/src/reader/pdf/assemble.rs
+++ b/crates/ariad-core/src/reader/pdf/assemble.rs
@@ -580,3 +580,7 @@
 const HYPHENS: [char; 3] = ['-', '\u{00AD}', '\u{2010}'];
 
+/// Longest word, in characters, that `line_join` reads left of a line-end hyphen. A longer run
+/// keeps its hyphen, so joining stays linear in the length of a block.
+const MAX_JOIN_WORD: usize = 64;
+
 /// How two lines meet.
@@ -593,15 +597,17 @@
 fn line_join(previous: &str, next: &str, vocabulary: &HashSet<String>) -> Join {
     let previous = previous.trim_end();
     let Some(stem) = previous.strip_suffix(HYPHENS) else {
         return Join::Space;
     };
-    let left = stem.rsplit(char::is_whitespace).next().unwrap_or_default();
-    let left = left.trim_start_matches(|ch: char| !ch.is_alphanumeric());
-    if !left.chars().last().is_some_and(char::is_alphanumeric) {
+    if !stem.chars().next_back().is_some_and(char::is_alphanumeric) {
         return Join::Space;
     }
     if previous.ends_with('\u{00AD}') {
         return Join::Merge;
     }
+    let Some(left) = last_word(stem) else {
+        return Join::Hyphen;
+    };
+    let left = left.trim_start_matches(|ch: char| !ch.is_alphanumeric());
     let right = next.split_whitespace().next().unwrap_or_default();
     // … the rest of the function (old :607-624) is unchanged …
 }
+
+/// The last whitespace-separated word of `text`, or `None` when it is longer than
+/// [`MAX_JOIN_WORD`] characters. Reads at most `MAX_JOIN_WORD + 1` characters.
+fn last_word(text: &str) -> Option<&str> {
+    let mut start = text.len();
+    for (count, (index, ch)) in text.char_indices().rev().enumerate() {
+        if ch.is_whitespace() {
+            break;
+        }
+        if count == MAX_JOIN_WORD {
+            return None;
+        }
+        start = index;
+    }
+    Some(&text[start..])
+}
```

Why this matches the old behaviour for words of up to 64 characters:

- `last_word` returns the same slice that `rsplit(char::is_whitespace).next()` returned.
- If the stem ends in an alphanumeric character, the last word ends in that same character, and `trim_start_matches` cannot remove it. If the stem does not, both versions return `Space`. So the alphanumeric test can move from `left` to `stem`.
- The soft-hyphen early return never read `left`.

The only behavioural change: a left word longer than 64 characters now returns `Hyphen` (keep the hyphen, no space) instead of being looked up and possibly merged.

**Regression tests.** Add them to `assemble.rs` `mod tests`, and change that module's first import to `use std::{collections::HashSet, time::{Duration, Instant}};`. The other imports (`Join`, `join_lines`, `line_join`, `span`) already exist (`:800-819`).

```rust
    #[test]
    fn hyphen_joins_read_at_most_64_characters_of_the_left_word() {
        let words = HashSet::new();
        let word = "a".repeat(64);
        assert_eq!(line_join(&format!("x {word}-"), "b", &words), Join::Merge);
        assert_eq!(line_join(&format!("{word}a-"), "b", &words), Join::Hyphen);
        assert_eq!(line_join(&format!("{word}a\u{00AD}"), "b", &words), Join::Merge);
        assert_eq!(line_join(&format!("{word}a -"), "b", &words), Join::Space);
    }

    #[test]
    fn long_runs_of_hyphenated_lines_join_in_linear_time() {
        // Every `ab-` line merged into the one before, so each join rescanned, copied and
        // lowercased the whole accumulated word: quadratic in the number of lines.
        const LINES: usize = 20_000;
        let lines = vec![vec![span("ab-")]; LINES];
        let started = Instant::now();
        let joined = join_lines(&lines, &HashSet::new());
        let elapsed = started.elapsed();
        assert!(elapsed < Duration::from_secs(1), "joining took {elapsed:?}");
        // Merging stops once the word passes 64 characters; later joins keep their hyphens.
        let expected = format!("{}{}-", "ab".repeat(33), "-ab".repeat(LINES - 33));
        assert_eq!(joined.len(), 1);
        assert_eq!(joined[0].text, expected);
    }
```

On origin/dev the second assertion of the first test fails at once, because the 65-character word merges. The second test runs for an estimated 10–20 s in debug (about 4e8 characters over four passes) and then fails both its time bound and its text check. The end-to-end version through `pdf::read` is in §3.6.

**Gate.** `cargo nextest run -p ariad-core -E 'test(reader::pdf::assemble)'`, then the §3.6 file, then `just clippy`, `just fmt-check` and `just wasm`.

**Commit.** `fix(core): bound PDF hyphen joining to a 64-character word`

**Risks.**

- A real word longer than 64 characters that is hyphenated across lines now keeps its hyphen. No fixture should contain one; `tests/pdf_reader.rs` re-checks the fixture text.
- The `vocabulary.contains(format!("{left}{right}"))` lookup still copies the next line's first word. That stays linear, because each line is `next` only once.

### 3.2 M8: every font walk over a shared `/Differences` or `/W` array is free

**Current behaviour.**

- `DocState::font_index` (`interpreter.rs:201-224`) caches by font object. Each distinct font object runs `Font::load` (`:212`) and is then charged only `work_bytes()` (`:215`, `font.rs:91-105`, about 21 KB).
- `/Differences` (`font.rs:366-378`) resolves every element. An integer resets `code` (`:370`), so a `[0 /a 0 /a …]` array is walked in full no matter how long it is.
- `/W` (`cid_widths`, `:456-513`) stops early only while it is pushing ranges (`:459`, `:468`). Triplets with `last < first` (`:489-493`) and non-numeric inner widths (`:477`) push nothing, so the whole array is walked.
- Thousands of fonts sharing one array therefore cost (number of fonts) × (array length) for linear input.

The missed variant is in `font.rs:465-482`: `array.get(index + 1)` is resolved, so a reference to one inner array counts as an inner width array. A single font whose `/W` is `[0 R 0 R …]`, with k entries naming one inner array of N names, walks k × N elements. About 300 KB of input then drives about 1e8 resolutions inside one font, which caching by font object cannot help.

**The exact change.** Charge every walk through the existing work budget. The unit is 16 bytes per element, the same as a glyph (`GLYPH_WORK`). That limits array walks across the whole document to (32 MiB + 64 B per input byte) / 16, which is linear in the input. The unit is deliberately small:

- At `size_of::<lopdf::Object>()` (an estimated 100–130 bytes), a real file with a few hundred font objects sharing one 300-element encoding could pass the 32 MiB base budget.
- At 16 bytes, that same file adds about 5 MB.

```diff
--- a/crates/ariad-core/src/reader/pdf/objects.rs
+++ b/crates/ariad-core/src/reader/pdf/objects.rs
@@ -77,3 +77,7 @@
 /// Work charged for each glyph the interpreter records, in bytes, on top of its text.
 pub(super) const GLYPH_WORK: u64 = 16;
 
+/// Work charged for each element of a font's `/Differences` or `/W` array, in bytes. Fonts
+/// that share an array walk it again, so every walk is charged.
+const ARRAY_ITEM_WORK: u64 = GLYPH_WORK;
+
@@ -137,2 +141,10 @@
     }
 
+    /// Charges a walk over `items` array elements.
+    pub(super) fn charge_items(&mut self, items: usize) -> Result<(), ReadError> {
+        self.work = self
+            .work
+            .saturating_add((items as u64).saturating_mul(ARRAY_ITEM_WORK));
+        self.exceeded()
+    }
+
     /// Decodes a stream within the remaining budget.
```

```diff
--- a/crates/ariad-core/src/reader/pdf/font.rs
+++ b/crates/ariad-core/src/reader/pdf/font.rs
@@ -261,3 +261,3 @@ (load_composite)
         if let Some(array) = get_array(doc, descendant, b"W") {
-            self.widths = Widths::Cid(cid_widths(doc, array));
+            self.widths = Widths::Cid(cid_widths(doc, array, budget)?);
         }
@@ -366,3 +366,4 @@ (load_simple)
         if let Some(differences) = differences {
+            budget.charge_items(differences.len())?;
             let mut code = 0_usize;
@@ -455,3 +456,10 @@
-/// Parses a CID font `/W` array into sorted ranges.
-fn cid_widths(doc: &Document, array: &[Object]) -> Vec<(u32, u32, f64)> {
+/// Parses a CID font `/W` array into sorted ranges, charging every element it walks, including
+/// inner width arrays that several entries may name.
+fn cid_widths(
+    doc: &Document,
+    array: &[Object],
+    budget: &mut Budget,
+) -> Result<Vec<(u32, u32, f64)>, ReadError> {
+    budget.charge_items(array.len())?;
     let mut ranges = Vec::new();
@@ -466,2 +474,3 @@
             Some(Object::Array(widths)) => {
+                budget.charge_items(widths.len())?;
                 for (offset, width) in widths.iter().enumerate() {
@@ -512,2 +521,2 @@
-    merged
+    Ok(merged)
 }
```

**Regression tests.** REPRO already specifies these two tests for `interpreter.rs` `mod tests` (§2.3, "`crates/ariad-core/src/reader/pdf/interpreter.rs`"). Use them unchanged:

- `shared_differences_arrays_are_charged_for_every_font_that_walks_them`: 200k elements × 16 B = 3.2 MB, past the 1 MiB limit at the first font.
- `rejected_cid_width_triplets_are_charged_for_every_font_that_walks_them`: 300k elements × 16 B = 4.8 MB, past 1 MiB at the first font.

On origin/dev, both return `None` instead of the error. Add this test for the missed variant to the same module (all imports already exist):

```rust
    #[test]
    fn cid_width_entries_naming_one_inner_array_charge_every_walk() {
        let mut doc = Document::new();
        // 1,000 `first [widths]` entries of one /W array all name the same 100,000-element inner
        // array. Names are not widths, so nothing is kept and the entry cap never stops the walk.
        let inner = doc.add_object(Object::Array(vec![Object::Name(b"w".to_vec()); 100_000]));
        let widths = (0..1_000)
            .flat_map(|_| [Object::Integer(0), Object::Reference(inner)])
            .collect::<Vec<_>>();
        let descendant = doc.add_object(dictionary! { "Subtype" => "CIDFontType2", "W" => widths });
        let font = doc.add_object(dictionary! {
            "Subtype" => "Type0",
            "Encoding" => "Identity-H",
            "DescendantFonts" => vec![Object::Reference(descendant)],
        });
        let limit = 1 << 20;
        let mut state = DocState::new(&doc, Budget::new(1 << 20, limit));
        assert_eq!(
            state.font_index(doc.get_object(font).unwrap()).err(),
            Some(super::ReadError::DecompressedTooLarge { limit })
        );
    }
```

On origin/dev this test walks 1e8 elements (a few seconds in debug), then returns `Ok(0)` and fails. With the fix, the first inner walk charges 1.6 MB and the test fails fast with the expected error.

Also relocate REPRO's fix-agnostic `fonts_sharing_one_large_differences_array_load_in_bounded_time` to the new file (§3.6). With a 16-byte unit, about 18 fonts × 400k elements (about 7M walks) use up the roughly 117 MB budget, so it fails fast with `DecompressedTooLarge`.

**Gate.** `cargo nextest run -p ariad-core -E 'test(reader::pdf)'`, then `cargo nextest run -p ariad-core --test pdf_reader --test pdf_reader_bounds`.

**Commit.** `fix(core): charge PDF font array walks against the work budget`

**Risks.**

- Each font's charge grows by 16 B per element of its `/Differences` and `/W` arrays. For subset fonts that means hundreds of elements; for unsubset CJK `/W` arrays, up to about 20k. A document with hundreds of font objects sharing one large unsubset `/W` gets closer to the budget. It still fits in the estimates above, but run the full `tests/pdf_reader.rs` fixture suite to confirm.
- `charge_items` charges each inner array's full length even when the range cap stops the walk early. That is conservative.

### 3.3 M9: parsed CMaps are not charged, and every font sharing a stream parses its own copy

**Current behaviour.**

- `Font::load` decodes and parses `/ToUnicode` for every font object (`font.rs:189-194`). `load_composite` does the same for an embedded encoding CMap (`:242-248`).
- `work_bytes` (`:91-105`) leaves out both CMaps, so the parsed heap is never charged. Only the decoded bytes are (`budget.decode`).
- A list `bfrange` keeps one `Vec<u16>` header (24 bytes) per `<>` target, which is 2 bytes of input (`cmap.rs:131-136`, `:226-240`). `push_range` accepts lists while `entries < MAX_ENTRIES` (`:145-162`), so one CMap keeps up to about 6.3 MB.
- Fonts sharing the stream each keep a copy, about 12× under-accounted (V-DC M9).

**The exact change.** Do three things:

1. Add `CMap::heap_bytes()`.
2. Parse each CMap stream once per document, behind an `Rc`, keyed by the stream's object id.
3. Charge `heap_bytes()` once, when the CMap is parsed.

Charging alone, per font, would make the reader refuse real files that keep many font objects sharing one large ToUnicode stream: the charge would rise about 4× per font. The cache keeps those files cheaper than today. Distinct streams still pay a full charge each, which is the case REPRO's test targets.

```diff
--- a/crates/ariad-core/src/reader/pdf/cmap.rs
+++ b/crates/ariad-core/src/reader/pdf/cmap.rs
@@ -167,2 +167,27 @@
     }
 
+    /// Approximate heap bytes the parsed CMap keeps: its tables, ranges and mapping targets.
+    pub(super) fn heap_bytes(&self) -> usize {
+        let unit = std::mem::size_of::<u16>();
+        let target = std::mem::size_of::<Vec<u16>>();
+        let singles = self.singles.capacity() * (std::mem::size_of::<(u8, u32)>() + target)
+            + self.singles.values().map(Vec::capacity).sum::<usize>() * unit;
+        let ranges = self.ranges.capacity() * std::mem::size_of::<Range>()
+            + self
+                .ranges
+                .iter()
+                .map(|range| match &range.target {
+                    RangeTarget::Increment(units) => units.capacity() * unit,
+                    RangeTarget::List(targets) => {
+                        targets.capacity() * target
+                            + targets.iter().map(Vec::capacity).sum::<usize>() * unit
+                    }
+                })
+                .sum::<usize>();
+        self.codespace.capacity() * std::mem::size_of::<CodespaceRange>() + singles + ranges
+    }
+
     /// Splits the next character code off `bytes` by the codespace ranges.
```

```diff
--- a/crates/ariad-core/src/reader/pdf/font.rs
+++ b/crates/ariad-core/src/reader/pdf/font.rs
@@ -1,4 +1,6 @@
 //! Font dictionaries: character code splitting, Unicode decoding and glyph widths.
 
-use lopdf::{Dictionary, Document, Object};
+use std::{collections::HashMap, rc::Rc};
+
+use lopdf::{Dictionary, Document, Object, ObjectId};
 
@@ -16,2 +18,5 @@
 const MAX_TYPE1_HEADER_BYTES: usize = 64 * 1024;
 
+/// Parsed CMaps by stream object, shared by every font that names the same stream.
+pub(super) type CMapCache = HashMap<ObjectId, Rc<CMap>>;
+
@@ enum Codes
-    Encoding(CMap),
+    Encoding(Rc<CMap>),
@@ struct Font
-    to_unicode: Option<CMap>,
+    to_unicode: Option<Rc<CMap>>,
@@ -90,1 +95,2 @@
-    /// Retained width/encoding allocations plus cache overhead, charged for every font.
+    /// Retained width/encoding allocations plus cache overhead, charged for every font. Parsed
+    /// CMaps are charged once, when `load_cmap` parses them.
@@ -180,17 +186,14 @@ (Font::load)
     pub(super) fn load(
         doc: &Document,
         dict: &Dictionary,
         budget: &mut Budget,
         encodings: &mut Encodings,
+        cmaps: &mut CMapCache,
     ) -> Result<Self, ReadError> {
         let mut font = Self::unknown();
         let subtype = get_name(doc, dict, b"Subtype").unwrap_or_default();
         let base_name = get_name(doc, dict, b"BaseFont").map(strip_subset_tag);
-        if let Some(stream) =
-            get(doc, dict, b"ToUnicode").and_then(|object| object.as_stream().ok())
-            && let Some(data) = budget.decode(stream)?
-        {
-            font.to_unicode = Some(CMap::parse(&data));
-        }
+        font.to_unicode = load_cmap(doc, dict, b"ToUnicode", budget, cmaps)?;
@@ (Font::load, composite branch)
-            font.load_composite(doc, dict, descendant, budget)?;
+            font.load_composite(doc, dict, descendant, budget, cmaps)?;
@@ (load_composite)
     fn load_composite(
         &mut self,
         doc: &Document,
         dict: &Dictionary,
         descendant: Option<&Dictionary>,
         budget: &mut Budget,
+        cmaps: &mut CMapCache,
     ) -> Result<(), ReadError> {
@@
                 if !identity
                     && !self.unicode_codes
-                    && self.to_unicode.as_ref().is_some_and(CMap::has_codespace)
+                    && self.to_unicode.as_deref().is_some_and(CMap::has_codespace)
                 {
@@
             Some(Object::Stream(stream)) => {
-                if let Some(data) = budget.decode(stream)? {
-                    let cmap = CMap::parse(&data);
-                    if cmap.has_codespace() {
-                        self.codes = Codes::Encoding(cmap);
-                    }
-                }
+                if let Some(cmap) = load_cmap(doc, dict, b"Encoding", budget, cmaps)?
+                    && cmap.has_codespace()
+                {
+                    self.codes = Codes::Encoding(cmap);
+                }
                 self.unicode_codes = get_name(doc, &stream.dict, b"CMapName")
@@ (new free function, e.g. after `is_useful`)
+/// Parses the CMap stream stored under `key`, once per stream object. A new parse is charged
+/// for its decoded bytes and for the heap the parsed CMap keeps.
+fn load_cmap(
+    doc: &Document,
+    dict: &Dictionary,
+    key: &[u8],
+    budget: &mut Budget,
+    cmaps: &mut CMapCache,
+) -> Result<Option<Rc<CMap>>, ReadError> {
+    let Ok(object) = dict.get(key) else {
+        return Ok(None);
+    };
+    let id = object.as_reference().ok();
+    if let Some(cmap) = id.and_then(|id| cmaps.get(&id)) {
+        return Ok(Some(Rc::clone(cmap)));
+    }
+    let Some(stream) = resolve(doc, object).and_then(|object| object.as_stream().ok()) else {
+        return Ok(None);
+    };
+    let Some(data) = budget.decode(stream)? else {
+        return Ok(None);
+    };
+    let cmap = Rc::new(CMap::parse(&data));
+    budget.charge_work(cmap.heap_bytes())?;
+    if let Some(id) = id {
+        cmaps.insert(id, Rc::clone(&cmap));
+    }
+    Ok(Some(cmap))
+}
@@ (font.rs mod tests)
-    use super::{Font, normalize_into, strip_subset_tag, style_from_name, type1_builtin_encoding};
+    use super::{
+        CMapCache, Font, normalize_into, strip_subset_tag, style_from_name, type1_builtin_encoding,
+    };
@@
         Font::load(
             doc,
             dict,
             &mut Budget::new(1 << 20, 1 << 20),
             &mut Encodings::default(),
+            &mut CMapCache::new(),
         )
```

`next_code` and `decode` need no change, because method calls auto-deref through `Rc` (`font.rs:116-121`, `:128`).

```diff
--- a/crates/ariad-core/src/reader/pdf/interpreter.rs
+++ b/crates/ariad-core/src/reader/pdf/interpreter.rs
@@ -17 +17 @@
-    font::{Font, normalize_into},
+    font::{CMapCache, Font, normalize_into},
@@ struct DocState
     encodings: Encodings,
+    cmaps: CMapCache,
@@ DocState::new
             encodings: Encodings::default(),
+            cmaps: CMapCache::new(),
@@ -212 +213,3 @@ (font_index)
-            Some(dict) => Font::load(self.doc, dict, &mut self.budget, &mut self.encodings)?,
+            Some(dict) => {
+                Font::load(self.doc, dict, &mut self.budget, &mut self.encodings, &mut self.cmaps)?
+            }
```

**Regression tests.** REPRO's `distinct_tounicode_cmaps_charge_their_parsed_size` (§2.3, `interpreter.rs`) stays valid unchanged:

- It uses 16 distinct streams, so the cache never hits.
- Each font is charged about 131 KB decoded, plus about 1.5 MiB of heap (65,536 × 24 B), plus about 21 KB, so the 8 MiB limit trips at the fifth font.
- On origin/dev the total is about 2.4 MB, so no error is raised and the test fails.

Add this guard test to the same module. It passes on origin/dev and with the fix, but it fails if anyone later replaces the cache with a plain per-font charge.

```rust
    #[test]
    fn fonts_sharing_one_tounicode_stream_parse_it_once() {
        let mut program = b"1 begincodespacerange <0000> <FFFF> endcodespacerange \
            1 beginbfrange <0000> <FFFF> ["
            .to_vec();
        program.extend(b"<>".repeat(65_536));
        program.extend_from_slice(b"] endbfrange");
        let mut doc = Document::new();
        let cmap = doc.add_object(Stream::new(Dictionary::new(), program));
        let ids = (0..16)
            .map(|_| {
                doc.add_object(dictionary! {
                    "Type" => "Font",
                    "Subtype" => "Type1",
                    "BaseFont" => "Helvetica",
                    "ToUnicode" => cmap,
                })
            })
            .collect::<Vec<_>>();
        // One parse keeps about 1.5 MiB and every font about 21 KB. Parsing the shared stream
        // again for each font would keep 16 copies, about 27 MB.
        let mut state = DocState::new(&doc, Budget::new(1 << 30, 4 << 20));
        for id in ids {
            assert!(state.font_index(doc.get_object(id).unwrap()).is_ok());
        }
    }
```

**Gate.** `cargo nextest run -p ariad-core -E 'test(reader::pdf)'`, then `--test pdf_reader`, then `just clippy` and `just wasm`.

**Commit.** `fix(core): charge parsed PDF CMaps once and share them by stream`

**Risks.**

- The cache keys on the first-level reference. A CMap stored as a direct object (malformed) is parsed and charged for each font, which is still correct.
- An encoding CMap without codespace ranges now stays cached for the whole document. It is charged.
- `heap_bytes` leaves out allocator overhead, such as the minimum allocation size for one-unit targets, and hash-table slack. It under-counts by at most about 1.5×, which is acceptable for an amplification budget.
- Shared ToUnicode streams are now decoded and charged once instead of once per font. No existing test depends on the per-font charge; I checked `tests/pdf_reader.rs` and the `reader/pdf` unit tests.

### 3.4 M10: operand arrays are capped per level but not in total

**Current behaviour.**

- `read_array` (`interpreter.rs:781-836`) caps each level at `MAX_ARRAY_ITEMS` (65,536, `:39`) and the depth at 16 (`:42`). It has no total cap.
- `run()` keeps up to `MAX_OPERANDS` (128, `:36`) operands per operator (`:362-366`).
- Every element becomes a 32-byte `Operand` (size per the DC/V-DC reports; not compile-checked here), so 2-byte `[]` elements amplify memory 16–32× until the operator runs.
- The interpreter charges only stream bytes (`:186`), fonts (`:215`) and glyphs (`:736`). DC estimates about 0.5–1 GB for 32 MB of decoded content.

**The exact change.** I recommend a fixed per-operator allowance of array elements instead of the master plan's "charge items × `size_of::<Operand>()`". A cumulative charge would count transient memory against the document-wide budget on every operator. Word-style content kerns per glyph (`[(H)3(e)-2(l)…] TJ`), which is about two elements per glyph, or about 64 bytes of new charge per glyph, against today's 17. At typical content compression ratios that is roughly 55–90 bytes of work per input byte, above the 64 B/byte budget, so large real PDFs could fail. A per-operator cap bounds peak memory at about 4 MiB (8 MiB with `Vec` slack) and changes nothing for valid content: a `TJ` array is already capped at 65,536 elements, and no operator takes two large arrays.

```diff
--- a/crates/ariad-core/src/reader/pdf/interpreter.rs
+++ b/crates/ariad-core/src/reader/pdf/interpreter.rs
@@ -41,2 +41,7 @@
 /// Deepest array nesting kept in one operand.
 const MAX_ARRAY_DEPTH: usize = 16;
 
+/// Most array elements kept across the operands of one operator, at every nesting level. This
+/// bounds operand memory per operator whatever shape the arrays take; elements past it are
+/// parsed and dropped.
+const MAX_OPERAND_ITEMS: usize = 2 * MAX_ARRAY_ITEMS;
+
@@ (interpret_page, Interpreter literal)
         operands: Vec::new(),
+        operand_items: 0,
         out: PageContent::default(),
@@ struct Interpreter
     operands: Vec<Operand>,
+    /// Array elements held by `operands`, at every nesting level.
+    operand_items: usize,
     out: PageContent,
@@ -361,7 +367,8 @@ (run)
                     other => {
-                        let operand = read_operand(other, &data, &mut pos);
                         if self.operands.len() >= MAX_OPERANDS {
-                            self.operands.clear();
+                            self.clear_operands();
                         }
+                        let operand =
+                            read_operand(other, &data, &mut pos, &mut self.operand_items);
                         self.operands.push(operand);
                         continue;
                     }
@@ -378 (BI branch)
-                    self.operands.clear();
+                    self.clear_operands();
@@ -385
-                self.operands.clear();
+                self.clear_operands();
@@ (new method in `impl Interpreter`, e.g. after `pop_frame`)
+    fn clear_operands(&mut self) {
+        self.operands.clear();
+        self.operand_items = 0;
+    }
@@ -765,6 +778,6 @@
-fn read_operand(token: Token<'_>, data: &[u8], pos: &mut usize) -> Operand {
+fn read_operand(token: Token<'_>, data: &[u8], pos: &mut usize, kept: &mut usize) -> Operand {
@@
-        Token::ArrayStart => read_array(data, pos),
+        Token::ArrayStart => read_array(data, pos, kept),
@@ -779,3 +792,5 @@
 /// Reads an array whose `[` was consumed. An operator inside the array closes it, so a missing
-/// `]` cannot swallow the rest of the stream.
-fn read_array(data: &[u8], pos: &mut usize) -> Operand {
+/// `]` cannot swallow the rest of the stream. `kept` counts the elements the operator's
+/// operands already hold; past [`MAX_OPERAND_ITEMS`] elements are parsed and dropped.
+fn read_array(data: &[u8], pos: &mut usize, kept: &mut usize) -> Operand {
@@ (ArrayEnd arm)
                     Some(parent) => {
-                        if parent.len() < MAX_ARRAY_ITEMS {
+                        if parent.len() < MAX_ARRAY_ITEMS && *kept < MAX_OPERAND_ITEMS {
                             parent.push(Operand::Array(done));
+                            *kept += 1;
                         }
                         continue;
                     }
@@ (after the match)
         if let Some(top) = stack.last_mut()
             && top.len() < MAX_ARRAY_ITEMS
+            && *kept < MAX_OPERAND_ITEMS
         {
             top.push(item);
+            *kept += 1;
         }
```

Moving the `MAX_OPERANDS` clear before `read_operand` changes nothing for operands, because `read_operand` never reads them. It does make the reset allowance apply to the operand being read. The closing loop (`:830-834`) may still add up to 15 enclosing arrays, which stays bounded.

**Regression test.** Add to `interpreter.rs` `mod tests`. It uses the existing `run` and `texts` helpers and compiles on origin/dev.

```rust
    #[test]
    fn array_elements_past_the_operator_allowance_are_dropped() {
        // Three arrays of 65,536 empty arrays stay under the per-level cap, yet one operator held
        // about 196,600 operands. Elements past the allowance are dropped, including the later
        // `[(x)]`, and the next operator starts with a fresh allowance.
        let mut content = b"BT /F1 10 Tf [".to_vec();
        for _ in 0..3 {
            content.push(b'[');
            content.extend(b"[]".repeat(super::MAX_ARRAY_ITEMS));
            content.push(b']');
        }
        content.extend_from_slice(b"] [(x)] TJ [(y)] TJ ET");
        assert_eq!(texts(&run(&content, |_, _| {})), "y");
    }
```

On origin/dev the result is `"xy"`; with the fix it is `"y"`. The content is about 393 KB, under the helper's 1 MiB budget.

**Gate.** `cargo nextest run -p ariad-core -E 'test(reader::pdf::interpreter)'`, then the whole `ariad-core` suite, because `positions_glyphs_with_text_state` and `malformed_operands_do_not_stop_the_page` cover `TJ`.

**Commit.** `fix(core): cap array elements kept per PDF content operator`

**Risks.**

- Malformed content that puts two or more large arrays before one operator loses elements past 131,072. Valid content never does.
- If the coordinator prefers the master plan's charge, use `budget.charge_work(items * size_of::<Operand>())`. That needs `read_array` to return its element count, and it carries the false-positive risk above.

### 3.5 Module doc overstates the guarantee

**Current behaviour.** `reader/pdf/mod.rs:8-15` says the amplification cap "bounds decoded bytes plus 16 bytes and the decoded text of every glyph, so a small file cannot expand into unbounded work or output". H7, M8, M9 and M10 contradict the second half. `MAX_OBJECTS` is checked only after `lopdf` has parsed the file (`mod.rs:145-148`).

**The exact change** (replace lines 8–15):

```rust
//! Work is bounded: the page count by [`Limits::max_pages`], decoded stream bytes by
//! [`Limits::max_decompressed_bytes`] (every use of a shared content stream is charged again),
//! each page by [`MAX_PAGE_OPERATORS`] operators and [`MAX_PAGE_GLYPHS`] glyphs, the array
//! elements one content operator keeps by a fixed cap, and form XObject nesting by
//! [`MAX_FORM_DEPTH`]. An amplification budget of [`CONTENT_BUDGET_BASE`] plus
//! [`CONTENT_BUDGET_PER_INPUT_BYTE`] per input byte is charged for decoded bytes, 16 bytes plus
//! the decoded text of every glyph, every element a font walks in its `/Differences` and `/W`
//! arrays, and the widths, encodings and parsed CMaps fonts keep. Exceeding either limit is
//! [`ReadError::DecompressedTooLarge`]. Documents declaring more than [`MAX_OBJECTS`] objects
//! are refused once `lopdf` has parsed them; that parse is bounded per stream by the
//! decompression limit, not by the amplification budget.
```

Do not edit `ARCHITECTURE.md` in this PR (master plan §4 I). Hand-off to I16: `ARCHITECTURE.md:705` says the reader caps "retained font allocations, and decoded/glyph work relative to input size". After I8 it should also mention font array walks, parsed CMaps and the per-operator operand cap.

**Gate.** `cargo doc -p ariad-core --no-deps`; the intra-doc links point only at public items. Then `just spell`.

**Commit.** `docs(core): state what the PDF reader budget covers`

### 3.6 End-to-end proving tests (new file)

Create `crates/ariad-core/tests/pdf_reader_bounds.rs`. It carries REPRO's two `tests/pdf_reader.rs` tests, with three changes:

- The file has its own helper.
- The H7 test sets the page height directly instead of editing the saved file.
- The H7 test asserts the one-paragraph precondition.

```rust
//! Resource bounds of the PDF reader on synthetic hostile files: each read ends in a typed
//! limit error or finishes within a time bound.
#![cfg(feature = "pdf")]

use std::time::{Duration, Instant};

use ariad_core::{
    limits::Limits,
    reader::pdf::{self, ReadError},
};
use lopdf::{Dictionary, Document, Object, Stream, dictionary};

/// A one-page PDF, `height` points tall, whose resources hold font `F1` plus what `extra` adds.
fn synthetic_pdf(
    content: &[u8],
    height: i64,
    extra: impl FnOnce(&mut Document, &mut Dictionary),
) -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let font = doc.add_object(
        dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" },
    );
    let mut resources = dictionary! { "Font" => dictionary! { "F1" => font } };
    extra(&mut doc, &mut resources);
    let stream = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), height.into()],
        "Resources" => resources,
        "Contents" => stream,
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1 },
        ),
    );
    let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", root);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn tall_page_of_hyphenated_lines_reads_in_bounded_time() {
    // 50,000 identical `ab-` lines at one x, size and pitch form one block. Each join used to
    // rescan, copy and lowercase the whole merged word.
    const LINES: usize = 50_000;
    let top = LINES * 6 / 5 + 10;
    let mut content = format!("BT /F1 1 Tf 1.2 TL 10 {top} Td").into_bytes();
    for _ in 0..LINES {
        content.extend_from_slice(b" (ab-) '");
    }
    content.extend_from_slice(b" ET");
    let height = i64::try_from(top + 20).unwrap();
    let bytes = synthetic_pdf(&content, height, |_, _| {});

    let started = Instant::now();
    let output = pdf::read(&bytes, &Limits::local()).expect("tall hyphenated page reads");
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(15),
        "hyphen joining took too long: {elapsed:?}"
    );
    assert_eq!(
        output.document.body.len(),
        1,
        "every line must stay in one paragraph, or the test no longer exercises joining"
    );
}

#[test]
fn fonts_sharing_one_large_differences_array_load_in_bounded_time() {
    // 3,000 fonts share one 400,000-element `/Differences` array. Integer entries reset the
    // code, so every font resolved every element: about 1.2e9 steps from about 1.4 MB of input.
    const FONTS: usize = 3_000;
    let mut content = b"BT".to_vec();
    for index in 0..FONTS {
        content.extend_from_slice(format!(" /G{index} 12 Tf").as_bytes());
    }
    content.extend_from_slice(b" 20 700 Td (x) Tj ET");
    let bytes = synthetic_pdf(&content, 792, |doc, resources| {
        let differences = (0..200_000)
            .flat_map(|_| [Object::Integer(0), Object::Name(b"a".to_vec())])
            .collect::<Vec<_>>();
        let differences = doc.add_object(Object::Array(differences));
        let mut fonts = Dictionary::new();
        for index in 0..FONTS {
            let font = doc.add_object(dictionary! {
                "Type" => "Font",
                "Subtype" => "Type1",
                "BaseFont" => "Helvetica",
                "Encoding" => dictionary! { "Differences" => differences },
            });
            fonts.set(format!("G{index}"), font);
        }
        resources.set("Font", fonts);
    });

    let started = Instant::now();
    let result = pdf::read(&bytes, &Limits::local());
    let elapsed = started.elapsed();
    assert!(
        matches!(result, Ok(_) | Err(ReadError::DecompressedTooLarge { .. })),
        "{:?}",
        result.as_ref().err()
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "shared /Differences took too long: {elapsed:?}"
    );
}
```

How these tests behave:

- **Before the fix**, both run for minutes in debug and then fail their time bound. H7 does about 2.5e9 characters per pass; M8 about 1.2e9 element walks. To confirm the failure without waiting, run `timeout 120 cargo nextest run -p ariad-core --test pdf_reader_bounds` and treat a timeout as a failure. There is no nextest timeout configuration (no `.config/nextest.toml`).
- **After the fix**, H7 is estimated at under 3 s in debug, about 150k glyphs through interpretation and layout. M8 stops with `DecompressedTooLarge` after about 7M element walks, estimated at 2–4 s in debug.

As REPRO §2.2 notes, record the real debug timings on the first green run and tighten each bound to about 5× headroom. The M8 bound of 10 s may need loosening if debug runs are slower than estimated. Do not remove that test.

**Gate.**

- `cargo nextest run -p ariad-core --test pdf_reader_bounds`.
- Before the PR: `just test`, `just clippy`, `just fmt-check`, `just wasm`, then `just ci`.
- On a Linux box: `just fuzz pdf_reader 60`.

**Commit.** `test(core): cover PDF reader bounds with synthetic files`. Alternatively, fold each test into the H7 and M8 commits.

## 4. Owner decisions needed

No OD from the master plan blocks I8.

The coordinator should confirm three changes from the master-plan wording before dispatch:

1. **H7:** bound only, without "join on the previous line's text" (§3.1).
2. **M9:** charge plus a per-stream `Rc` cache, not one or the other (§3.3).
3. **M10:** a per-operator element cap instead of a cumulative charge (§3.4).

Each section gives the evidence and the fallback. One hand-off: I16 should update `ARCHITECTURE.md:705` after step 3b (§3.5).

## 5. Unresolved questions

1. Every time bound and runtime is an estimate from operation counts. Nothing was compiled or run. That includes `size_of::<Operand>() == 32` and the about 21 KB per-font `work_bytes`.
2. Master plan question 8 (DC Q2) is still open: do local surfaces run the isolated PDF engine with `timeout_s` and `max_memory_mb` unset? That decides whether these defects hang or get killed today. It changes severity, not the fix.
3. Should `/Differences` overlays and `/W` ranges also be cached by object id, as CMaps now are? It is not needed for the bound. It would only reduce work and charges for real files that repeat font objects.
4. Group J item J3 (DC L8–L10: xref `/Size` pre-check, page clipping, the `partition_point` order) touches `reader/pdf/{mod,layout}.rs`. It lands after I8, has not been re-verified, and is not specified here.

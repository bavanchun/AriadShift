//! GID-preserving pruning of bare, name-keyed CFF programs (`/FontFile3 /Type1C`).
//!
//! Unused charstrings become a lone `endchar`, so glyph ids, the charset, the
//! built-in encoding, every subroutine and the PDF `/Widths` stay valid. Only
//! the `CharStrings` INDEX changes. The Top DICT is rewritten with fixed-width
//! offset operands so the new layout is computed in a single pass. Anything
//! unexpected makes the pruner return `None` and the font stays untouched.

use super::{
    FontError, GlyphUsage, SubsetOutcome, content_bytes, dict, resolve, shared_font_program,
};
use crate::{PdfDoc, PdfError, ToolCtx};
use ariad_core::limits::Limits;
use flate2::{Compression, write::ZlibEncoder};
use lopdf::{Object, ObjectId, Stream, dictionary};
use read_fonts::ps::{PredefinedEncoding, Sid, agl, cff::CffFontRef};
use std::{collections::BTreeSet, io::Write};

const ENDCHAR: u8 = 14;
const MAX_STACK: usize = 192;
const MAX_CALL_DEPTH: u8 = 10;
const MAX_CALLS: usize = 4096;
const MAX_STEMS: usize = 96;
const MAX_SEAC_ROUNDS: usize = 8;
/// Interpreted bytes allowed per program byte, with a floor and a fixed cap.
const WORK_PER_BYTE: usize = 16;
const WORK_FLOOR: usize = 256 * 1024;
const WORK_CAP: usize = 32 * 1024 * 1024;
/// Interpreted bytes between two cancellation checks.
const CHECK_EVERY: usize = 64 * 1024;

/// Glyph names and the built-in encoding of a name-keyed CFF program.
#[derive(Debug, PartialEq, Eq)]
struct Face {
    /// Glyph name by glyph id.
    names: Vec<Vec<u8>>,
    /// Glyph id for each character code in the built-in encoding.
    builtin: Vec<Option<u16>>,
    /// The built-in encoding is the predefined Standard or Expert one.
    predefined_encoding: bool,
}

fn parse(bytes: &[u8]) -> Option<Face> {
    let font = CffFontRef::new_cff(bytes, 0, None).ok()?;
    if font.is_cid() {
        return None;
    }
    let charset = font.charset()?;
    let count = usize::try_from(font.num_glyphs()).ok()?;
    let mut names = vec![Vec::new(); count];
    let mut seen = 0;
    for (gid, sid) in charset.iter() {
        let slot = names.get_mut(usize::try_from(gid.to_u32()).ok()?)?;
        *slot = font.string(sid)?.to_vec();
        seen += 1;
    }
    // Glyph 0 is implicit in the charset iterator of some formats.
    if seen != count && seen + 1 != count {
        return None;
    }
    if names.first().is_none_or(Vec::is_empty) {
        *names.first_mut()? = b".notdef".to_vec();
    }
    let encoding = font.encoding()?;
    let builtin = (0..=255_u8)
        .map(|code| {
            encoding
                .map(code)
                .and_then(|g| u16::try_from(g.to_u32()).ok())
                .filter(|g| *g != 0)
        })
        .collect();
    Some(Face {
        names,
        builtin,
        predefined_encoding: encoding.predefined().is_some(),
    })
}

fn char_of(name: &[u8]) -> Option<char> {
    agl::name_to_char(std::str::from_utf8(name).ok()?)
}

/// Glyph ids that carry `name`, or a name that maps to `ch` (default `name`'s own value).
fn glyphs_matching(face: &Face, name: Option<&[u8]>, ch: Option<char>) -> BTreeSet<u16> {
    let wanted = ch.or_else(|| name.and_then(char_of));
    face.names
        .iter()
        .enumerate()
        .filter(|(_, glyph)| {
            name == Some(glyph.as_slice()) || wanted.is_some() && char_of(glyph) == wanted
        })
        .filter_map(|(gid, _)| u16::try_from(gid).ok())
        .collect()
}

/// Glyph ids a viewer may pick for a Unicode character.
fn glyphs_for_char(face: &Face, ch: char) -> BTreeSet<u16> {
    let mut found = glyphs_matching(face, None, Some(ch));
    // Viewers fold these two spaces and hyphens onto the plain glyphs.
    let fold: &[&[u8]] = match ch {
        '\u{a0}' => &[b"space"],
        '\u{ad}' => &[b"hyphen"],
        _ => &[],
    };
    for name in fold {
        found.extend(glyphs_matching(face, Some(name), None));
    }
    found
}

/// A parsed CFF INDEX: element offsets are absolute positions in the program.
struct Index {
    count: usize,
    offsets: Vec<usize>,
    end: usize,
}

fn be_u16(data: &[u8], pos: usize) -> Option<usize> {
    Some(usize::from(u16::from_be_bytes([
        *data.get(pos)?,
        *data.get(pos.checked_add(1)?)?,
    ])))
}

fn parse_index(data: &[u8], pos: usize) -> Option<Index> {
    let count = be_u16(data, pos)?;
    if count == 0 {
        return Some(Index {
            count,
            offsets: Vec::new(),
            end: pos.checked_add(2)?,
        });
    }
    let size = usize::from(*data.get(pos.checked_add(2)?)?);
    if !(1..=4).contains(&size) {
        return None;
    }
    let table = pos.checked_add(3)?;
    let base = table
        .checked_add((count + 1).checked_mul(size)?)?
        .checked_sub(1)?;
    let mut offsets = Vec::with_capacity(count + 1);
    let mut previous = 1;
    for i in 0..=count {
        let at = table.checked_add(i * size)?;
        let raw = data.get(at..at.checked_add(size)?)?;
        let value = raw
            .iter()
            .fold(0_usize, |acc, b| (acc << 8) | usize::from(*b));
        if value < previous {
            return None;
        }
        previous = value;
        offsets.push(base.checked_add(value)?);
    }
    if offsets.first() != Some(&(base + 1)) {
        return None;
    }
    let end = *offsets.last()?;
    if end > data.len() {
        return None;
    }
    Some(Index {
        count,
        offsets,
        end,
    })
}

fn element<'a>(data: &'a [u8], index: &Index, i: usize) -> Option<&'a [u8]> {
    data.get(*index.offsets.get(i)?..*index.offsets.get(i + 1)?)
}

fn build_index(elements: &[&[u8]]) -> Option<Vec<u8>> {
    let count = u16::try_from(elements.len()).ok()?;
    let mut out = count.to_be_bytes().to_vec();
    if elements.is_empty() {
        return Some(out);
    }
    let total: usize = elements.iter().map(|e| e.len()).sum();
    let last = total.checked_add(1)?;
    let size = match last {
        0..=0xff => 1,
        0x100..=0xffff => 2,
        0x1_0000..=0xff_ffff => 3,
        _ => 4,
    };
    out.push(u8::try_from(size).ok()?);
    let push = |out: &mut Vec<u8>, value: usize| {
        out.extend_from_slice(&u32::try_from(value).unwrap_or(u32::MAX).to_be_bytes()[4 - size..]);
    };
    let mut offset = 1_usize;
    push(&mut out, offset);
    for element in elements {
        offset += element.len();
        push(&mut out, offset);
    }
    for element in elements {
        out.extend_from_slice(element);
    }
    Some(out)
}

/// One DICT entry: its raw operand encodings, their integer value and the operator.
#[derive(Clone)]
struct Entry {
    operands: Vec<(Vec<u8>, Option<i32>)>,
    operator: u16,
}

fn operand(data: &[u8], pos: &mut usize) -> Option<(Vec<u8>, Option<i32>)> {
    let start = *pos;
    let b0 = i32::from(*data.get(start)?);
    let next = |i: usize| data.get(start + i).copied().map(i32::from);
    let (len, value) = match b0 {
        28 => (
            3,
            Some(i32::from(i16::from_be_bytes([
                *data.get(start + 1)?,
                *data.get(start + 2)?,
            ]))),
        ),
        29 => {
            let raw = data.get(start + 1..start + 5)?;
            (
                5,
                Some(i32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]])),
            )
        }
        32..=246 => (1, Some(b0 - 139)),
        247..=250 => (2, Some((b0 - 247) * 256 + next(1)? + 108)),
        251..=254 => (2, Some(-(b0 - 251) * 256 - next(1)? - 108)),
        30 => {
            let mut at = start + 1;
            loop {
                let byte = *data.get(at)?;
                at += 1;
                if byte & 0x0f == 0x0f || byte >> 4 == 0x0f {
                    break;
                }
            }
            (at - start, None)
        }
        _ => return None,
    };
    *pos = start + len;
    Some((data.get(start..*pos)?.to_vec(), value))
}

fn parse_dict(data: &[u8]) -> Option<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut operands = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        let b0 = data[pos];
        if b0 <= 21 {
            pos += 1;
            let operator = if b0 == 12 {
                let second = *data.get(pos)?;
                pos += 1;
                0x0c00 | u16::from(second)
            } else {
                u16::from(b0)
            };
            entries.push(Entry {
                operands: std::mem::take(&mut operands),
                operator,
            });
        } else {
            operands.push(operand(data, &mut pos)?);
        }
    }
    operands.is_empty().then_some(entries)
}

fn fixed(value: usize) -> Option<Vec<u8>> {
    let mut out = vec![29];
    out.extend_from_slice(&i32::try_from(value).ok()?.to_be_bytes());
    Some(out)
}

fn emit_dict(entries: &[Entry]) -> Vec<u8> {
    let mut out = Vec::new();
    for entry in entries {
        for (raw, _) in &entry.operands {
            out.extend_from_slice(raw);
        }
        if entry.operator >= 0x0c00 {
            out.extend_from_slice(&[12, (entry.operator & 0xff) as u8]);
        } else {
            out.push(entry.operator as u8);
        }
    }
    out
}

fn subr_bias(count: usize) -> i32 {
    match count {
        0..=1239 => 107,
        1240..=33899 => 1131,
        _ => 32768,
    }
}

/// Subroutine INDEXes a charstring may call.
struct Subrs<'a> {
    data: &'a [u8],
    globals: &'a Index,
    locals: Option<&'a Index>,
}

enum Step {
    /// An `endchar` ran, optionally with the deprecated four-argument accent form.
    Ended(Option<(u8, u8)>),
    /// The charstring or subroutine ran out of operators or hit `return`.
    Returned,
}

/// Minimal Type 2 interpreter: it only follows the operand stack, the hint
/// count and subroutine calls, which is enough to find a `seac`-style `endchar`.
///
/// One machine serves a whole font. The interpreted bytes of every charstring
/// and subroutine call are charged to a font-wide budget, so a few kilobytes
/// of nested subroutines cannot cost seconds; running dry refuses the font.
struct Machine<'a> {
    subrs: &'a Subrs<'a>,
    ctx: &'a ToolCtx,
    stack: Vec<i32>,
    stems: usize,
    calls: usize,
    /// Interpreted bytes still allowed for the font.
    budget: usize,
    until_check: usize,
    /// Set when the context asked to stop; the font is then not an outcome.
    stopped: Option<PdfError>,
}

impl Machine<'_> {
    /// Interpret one glyph from a clean state.
    fn glyph(&mut self, code: &[u8]) -> Option<Step> {
        self.stack.clear();
        self.stems = 0;
        self.calls = 0;
        self.run(code, 0)
    }

    fn charge(&mut self, bytes: usize) -> Option<()> {
        self.budget = self.budget.checked_sub(bytes)?;
        self.until_check = self.until_check.saturating_sub(bytes);
        if self.until_check == 0 {
            self.until_check = CHECK_EVERY;
            if let Err(error) = self.ctx.check() {
                self.stopped = Some(error);
                return None;
            }
        }
        Some(())
    }

    fn run(&mut self, code: &[u8], depth: u8) -> Option<Step> {
        self.charge(code.len())?;
        let mut pos = 0;
        while pos < code.len() {
            let b0 = code[pos];
            pos += 1;
            match b0 {
                28 => {
                    let raw = code.get(pos..pos + 2)?;
                    self.push(i32::from(i16::from_be_bytes([raw[0], raw[1]])))?;
                    pos += 2;
                }
                32..=246 => self.push(i32::from(b0) - 139)?,
                247..=250 => {
                    let b1 = i32::from(*code.get(pos)?);
                    self.push((i32::from(b0) - 247) * 256 + b1 + 108)?;
                    pos += 1;
                }
                251..=254 => {
                    let b1 = i32::from(*code.get(pos)?);
                    self.push(-(i32::from(b0) - 251) * 256 - b1 - 108)?;
                    pos += 1;
                }
                255 => {
                    let raw = code.get(pos..pos + 4)?;
                    self.push(i32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]) >> 16)?;
                    pos += 4;
                }
                1 | 3 | 18 | 23 => self.add_stems()?,
                19 | 20 => {
                    self.add_stems()?;
                    pos = pos.checked_add(self.stems.div_ceil(8))?;
                    if pos > code.len() {
                        return None;
                    }
                }
                4..=8 | 21 | 22 | 24..=27 | 30 | 31 => self.stack.clear(),
                12 => {
                    // dotsection and the four flex operators; arithmetic is refused.
                    if !matches!(*code.get(pos)?, 0 | 34..=37) {
                        return None;
                    }
                    pos += 1;
                    self.stack.clear();
                }
                10 | 29 => {
                    let index = if b0 == 10 {
                        self.subrs.locals?
                    } else {
                        self.subrs.globals
                    };
                    let number = self.stack.pop()?.checked_add(subr_bias(index.count))?;
                    let subr = element(self.subrs.data, index, usize::try_from(number).ok()?)?;
                    self.calls += 1;
                    if depth >= MAX_CALL_DEPTH || self.calls > MAX_CALLS {
                        return None;
                    }
                    if let Step::Ended(seac) = self.run(subr, depth + 1)? {
                        return Some(Step::Ended(seac));
                    }
                }
                11 => return Some(Step::Returned),
                ENDCHAR => {
                    let len = self.stack.len();
                    let seac = match len {
                        0 | 1 => None,
                        4 | 5 => {
                            let bchar = u8::try_from(self.stack[len - 2]).ok()?;
                            let achar = u8::try_from(self.stack[len - 1]).ok()?;
                            Some((bchar, achar))
                        }
                        _ => return None,
                    };
                    return Some(Step::Ended(seac));
                }
                _ => return None,
            }
        }
        Some(Step::Returned)
    }

    fn push(&mut self, value: i32) -> Option<()> {
        if self.stack.len() >= MAX_STACK {
            return None;
        }
        self.stack.push(value);
        Some(())
    }

    // Hint operators take pairs from the stack; a leading odd value is the width.
    fn add_stems(&mut self) -> Option<()> {
        self.stems += self.stack.len() / 2;
        self.stack.clear();
        (self.stems <= MAX_STEMS).then_some(())
    }
}

fn seac_glyph(font: &CffFontRef, code: u8) -> Option<u16> {
    let sid: Sid = PredefinedEncoding::Standard.sid(code)?;
    u16::try_from(font.charset()?.glyph_id(sid)?.to_u32()).ok()
}

/// Returns the pruned program, or `None` when it cannot be rewritten safely.
///
/// `Err` only reports that `ctx` cancelled the work or ran out of time.
fn prune(bytes: &[u8], keep: &BTreeSet<u16>, ctx: &ToolCtx) -> Result<Option<Vec<u8>>, PdfError> {
    prune_inner(bytes, keep, ctx).unwrap_or(Ok(None))
}

fn prune_inner(
    bytes: &[u8],
    keep: &BTreeSet<u16>,
    ctx: &ToolCtx,
) -> Option<Result<Option<Vec<u8>>, PdfError>> {
    let font = CffFontRef::new_cff(bytes, 0, None).ok()?;
    if font.is_cid() || *bytes.first()? != 1 || bytes.len() > u32::MAX as usize / 2 {
        return None;
    }
    let num_glyphs = usize::try_from(font.num_glyphs()).ok()?;
    let names = parse_index(bytes, usize::from(*bytes.get(2)?))?;
    let tops = parse_index(bytes, names.end)?;
    if tops.count != 1 {
        return None;
    }
    let strings = parse_index(bytes, tops.end)?;
    let globals = parse_index(bytes, strings.end)?;
    let (top_start, top_end) = (names.end, tops.end);
    let mut entries = parse_dict(element(bytes, &tops, 0)?)?;
    let mut charstrings_at = None;
    let mut private = None;
    let mut referenced = Vec::new();
    for entry in &entries {
        let value = |i: usize| entry.operands.get(i).and_then(|o| o.1);
        match entry.operator {
            // ROS, FDArray, FDSelect and SyntheticBase belong to CID or synthetic fonts.
            0x0c1e | 0x0c24 | 0x0c25 | 0x0c14 => return None,
            0x0c06 if value(0) != Some(2) => return None,
            17 => charstrings_at = Some(usize::try_from(value(0)?).ok()?),
            18 => {
                private = Some((
                    usize::try_from(value(0)?).ok()?,
                    usize::try_from(value(1)?).ok()?,
                ));
            }
            // Values 0..=2 (and 0..=1 for Encoding) select predefined tables.
            15 if value(0)? > 2 => referenced.push(usize::try_from(value(0)?).ok()?),
            16 if value(0)? > 1 => referenced.push(usize::try_from(value(0)?).ok()?),
            _ => {}
        }
    }
    let cs_off = charstrings_at?;
    let cs = parse_index(bytes, cs_off)?;
    if cs.count != num_glyphs || num_glyphs == 0 || cs_off < top_end {
        return None;
    }
    let cs_end = cs.end;
    let mut local_subrs = None;
    if let Some((size, offset)) = private {
        let dict = parse_dict(bytes.get(offset..offset.checked_add(size)?)?)?;
        for entry in dict {
            if entry.operator == 19 {
                let relative = usize::try_from(entry.operands.first()?.1?).ok()?;
                local_subrs = Some(offset.checked_add(relative)?);
            }
        }
        // Private DICT and its Subrs keep their relative distance only if both
        // sit on the same side of the CharStrings INDEX; a Private DICT without
        // Subrs has no distance to keep.
        if local_subrs.is_some_and(|s| (s >= cs_end) != (offset >= cs_end)) {
            return None;
        }
        referenced.push(offset);
    }
    let outside = |o: usize| o >= top_end && !(cs_off..cs_end).contains(&o);
    if !referenced.iter().all(|&o| outside(o)) || !local_subrs.is_none_or(outside) {
        return None;
    }
    let locals = match local_subrs {
        Some(at) => Some(parse_index(bytes, at)?),
        None => None,
    };
    let subrs = Subrs {
        data: bytes,
        globals: &globals,
        locals: locals.as_ref(),
    };
    // Keep the requested glyphs, glyph 0 and the accent parts of each. Each
    // round interprets only the glyphs the previous one added.
    let mut kept: BTreeSet<u16> = keep.iter().copied().chain([0]).collect();
    let mut pending: Vec<u16> = kept.iter().copied().collect();
    let budget = bytes
        .len()
        .saturating_mul(WORK_PER_BYTE)
        .clamp(WORK_FLOOR, WORK_CAP);
    let mut machine = Machine {
        subrs: &subrs,
        ctx,
        stack: Vec::new(),
        stems: 0,
        calls: 0,
        budget,
        until_check: CHECK_EVERY,
        stopped: None,
    };
    for _ in 0..MAX_SEAC_ROUNDS {
        let mut added = Vec::new();
        for gid in pending {
            let code = element(bytes, &cs, usize::from(gid))?;
            let Some(step) = machine.glyph(code) else {
                return machine.stopped.take().map(Err);
            };
            if let Step::Ended(Some((base, accent))) = step {
                for part in [base, accent] {
                    let part = seac_glyph(&font, part)?;
                    if kept.insert(part) {
                        added.push(part);
                    }
                }
            }
        }
        pending = added;
        if pending.is_empty() {
            break;
        }
    }
    if !pending.is_empty()
        || kept.iter().any(|&g| usize::from(g) >= num_glyphs)
        || kept.len() >= num_glyphs
    {
        return None;
    }
    let replacement = [ENDCHAR];
    let mut pruned: Vec<&[u8]> = Vec::with_capacity(num_glyphs);
    for gid in 0..num_glyphs {
        let original = element(bytes, &cs, gid)?;
        let wanted = u16::try_from(gid).is_ok_and(|g| kept.contains(&g));
        pruned.push(if wanted { original } else { &replacement });
    }
    let new_cs = build_index(&pruned)?;
    // Offset operands get a fixed width, so the Top DICT size is known up front.
    let is_offset = |entry: &Entry| -> Option<usize> {
        let old = usize::try_from(entry.operands.last()?.1?).ok()?;
        match entry.operator {
            15 if old > 2 => Some(old),
            16 if old > 1 => Some(old),
            17 | 18 => Some(old),
            _ => None,
        }
    };
    let offsets: Vec<Option<usize>> = entries.iter().map(is_offset).collect();
    let apply = |entries: &mut [Entry], value: &dyn Fn(usize) -> usize| -> Option<()> {
        for (entry, old) in entries.iter_mut().zip(&offsets) {
            if let Some(old) = old {
                *entry.operands.last_mut()? = (fixed(value(*old))?, None);
            }
        }
        Some(())
    };
    apply(&mut entries, &|_| 0)?;
    let dict_len = emit_dict(&entries).len();
    let new_top_len = build_index(&[&vec![0; dict_len]])?.len();
    let old_top_len = top_end - top_start;
    let (new_cs_len, old_cs_len) = (new_cs.len(), cs_end - cs_off);
    apply(&mut entries, &|offset| {
        let moved = offset + new_top_len - old_top_len;
        if offset >= cs_end {
            moved + new_cs_len - old_cs_len
        } else {
            moved
        }
    })?;
    let new_dict = emit_dict(&entries);
    let new_top = build_index(&[&new_dict])?;
    if new_dict.len() != dict_len || new_top.len() != new_top_len {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(&bytes[..top_start]);
    out.extend_from_slice(&new_top);
    out.extend_from_slice(&bytes[top_end..cs_off]);
    out.extend_from_slice(&new_cs);
    out.extend_from_slice(&bytes[cs_end..]);
    Some(Ok((out.len() < bytes.len()
        && verify(bytes, &out, &kept, cs_end))
    .then_some(out)))
}

/// Re-parse the output and compare it with the input before it replaces anything.
fn verify(old: &[u8], new: &[u8], kept: &BTreeSet<u16>, old_cs_end: usize) -> bool {
    let (Some(a), Some(b)) = (parse(old), parse(new)) else {
        return false;
    };
    if a != b {
        return false;
    }
    let (Some(x), Some(y)) = (layout(old), layout(new)) else {
        return false;
    };
    if x.count != y.count || x.private != y.private {
        return false;
    }
    let same_glyphs = (0..x.count).all(|g| {
        let (Some(before), Some(after)) = (element(old, &x.cs, g), element(new, &y.cs, g)) else {
            return false;
        };
        if u16::try_from(g).is_ok_and(|g| kept.contains(&g)) {
            before == after
        } else {
            after == [ENDCHAR]
        }
    });
    // Everything outside the CharStrings INDEX is copied verbatim; the Top DICT is
    // the only region allowed to differ and it must still resolve to the same data.
    let head_end = x.top_end;
    same_glyphs
        && old.get(old_cs_end..) == new.get(y.cs.end..)
        && old.get(head_end..x.cs_off) == new.get(y.top_end..y.cs_off)
        && old.get(..x.top_start) == new.get(..y.top_start)
}

struct Layout {
    count: usize,
    cs: Index,
    cs_off: usize,
    top_start: usize,
    top_end: usize,
    private: Option<(usize, Vec<u8>)>,
}

fn layout(data: &[u8]) -> Option<Layout> {
    let names = parse_index(data, usize::from(*data.get(2)?))?;
    let tops = parse_index(data, names.end)?;
    let entries = parse_dict(element(data, &tops, 0)?)?;
    let (mut cs_off, mut private) = (None, None);
    for entry in entries {
        let v = |i: usize| entry.operands.get(i).and_then(|o| o.1);
        match entry.operator {
            17 => cs_off = Some(usize::try_from(v(0)?).ok()?),
            18 => {
                let (size, at) = (usize::try_from(v(0)?).ok()?, usize::try_from(v(1)?).ok()?);
                private = Some((size, data.get(at..at.checked_add(size)?)?.to_vec()));
            }
            _ => {}
        }
    }
    let cs_off = cs_off?;
    let cs = parse_index(data, cs_off)?;
    Some(Layout {
        count: cs.count,
        cs,
        cs_off,
        top_start: names.end,
        top_end: tops.end,
        private,
    })
}

fn flate_len(bytes: &[u8]) -> Option<usize> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(bytes).ok()?;
    Some(encoder.finish().ok()?.len())
}

type CharTable = [Option<char>; 256];

/// Characters of a predefined PDF base encoding, as lopdf decodes them.
fn base_table(pdf: &lopdf::Document, name: &[u8]) -> Option<CharTable> {
    if !matches!(
        name,
        b"WinAnsiEncoding" | b"MacRomanEncoding" | b"MacExpertEncoding" | b"StandardEncoding"
    ) {
        return None;
    }
    let font = dictionary! {"Type" => "Font", "Encoding" => Object::Name(name.to_vec())};
    let encoding = font.get_font_encoding(pdf).ok()?;
    let mut table = [None; 256];
    for code in 0..=255_u8 {
        let mut chars = encoding
            .bytes_to_string(&[code])
            .ok()?
            .chars()
            .collect::<Vec<_>>();
        if chars.len() == 1 && chars[0] != '\u{fffd}' {
            table[usize::from(code)] = chars.pop();
        }
    }
    Some(table)
}

type Names = [&'static str; 256];

const WIN_ANSI_HIGH: [&str; 32] = [
    "Euro",
    "bullet",
    "quotesinglbase",
    "florin",
    "quotedblbase",
    "ellipsis",
    "dagger",
    "daggerdbl",
    "circumflex",
    "perthousand",
    "Scaron",
    "guilsinglleft",
    "OE",
    "bullet",
    "Zcaron",
    "bullet",
    "bullet",
    "quoteleft",
    "quoteright",
    "quotedblleft",
    "quotedblright",
    "bullet",
    "endash",
    "emdash",
    "tilde",
    "trademark",
    "scaron",
    "guilsinglright",
    "oe",
    "bullet",
    "zcaron",
    "Ydieresis",
];

const MAC_ROMAN_HIGH: [&str; 128] = [
    "Adieresis",
    "Aring",
    "Ccedilla",
    "Eacute",
    "Ntilde",
    "Odieresis",
    "Udieresis",
    "aacute",
    "agrave",
    "acircumflex",
    "adieresis",
    "atilde",
    "aring",
    "ccedilla",
    "eacute",
    "egrave",
    "ecircumflex",
    "edieresis",
    "iacute",
    "igrave",
    "icircumflex",
    "idieresis",
    "ntilde",
    "oacute",
    "ograve",
    "ocircumflex",
    "odieresis",
    "otilde",
    "uacute",
    "ugrave",
    "ucircumflex",
    "udieresis",
    "dagger",
    "degree",
    "cent",
    "sterling",
    "section",
    "bullet",
    "paragraph",
    "germandbls",
    "registered",
    "copyright",
    "trademark",
    "acute",
    "dieresis",
    "notequal",
    "AE",
    "Oslash",
    "infinity",
    "plusminus",
    "lessequal",
    "greaterequal",
    "yen",
    "mu",
    "partialdiff",
    "summation",
    "product",
    "pi",
    "integral",
    "ordfeminine",
    "ordmasculine",
    "Omega",
    "ae",
    "oslash",
    "questiondown",
    "exclamdown",
    "logicalnot",
    "radical",
    "florin",
    "approxequal",
    "Delta",
    "guillemotleft",
    "guillemotright",
    "ellipsis",
    "space",
    "Agrave",
    "Atilde",
    "Otilde",
    "OE",
    "oe",
    "endash",
    "emdash",
    "quotedblleft",
    "quotedblright",
    "quoteleft",
    "quoteright",
    "divide",
    "lozenge",
    "ydieresis",
    "Ydieresis",
    "fraction",
    "currency",
    "guilsinglleft",
    "guilsinglright",
    "fi",
    "fl",
    "daggerdbl",
    "periodcentered",
    "quotesinglbase",
    "quotedblbase",
    "perthousand",
    "Acircumflex",
    "Ecircumflex",
    "Aacute",
    "Edieresis",
    "Egrave",
    "Iacute",
    "Icircumflex",
    "Idieresis",
    "Igrave",
    "Oacute",
    "Ocircumflex",
    "apple",
    "Ograve",
    "Uacute",
    "Ucircumflex",
    "Ugrave",
    "dotlessi",
    "circumflex",
    "tilde",
    "macron",
    "breve",
    "dotaccent",
    "ring",
    "cedilla",
    "hungarumlaut",
    "ogonek",
    "caron",
];

/// Glyph names of the predefined PDF base encodings (PDF 32000-1 Annex D), so
/// names outside the Adobe Glyph List, such as MacRoman `apple`, match by name.
/// MacExpert has no table here and falls back to character matching.
fn base_names(name: &[u8]) -> Option<Names> {
    let latin = |code: u8| match code {
        // The CFF ISOLatin1 table has quoteleft here; the PDF ones have grave.
        0x60 => "grave",
        _ => PredefinedEncoding::IsoLatin1.name(code),
    };
    let mut table = [""; 256];
    for code in 0..=255_u8 {
        let slot = usize::from(code);
        table[slot] = match name {
            b"StandardEncoding" => PredefinedEncoding::Standard.name(code),
            b"WinAnsiEncoding" => match code {
                0x7f => "bullet",
                0x80..=0x9f => WIN_ANSI_HIGH[slot - 0x80],
                0xa0 => "space",
                0xad => "hyphen",
                _ => latin(code),
            },
            b"MacRomanEncoding" => match code {
                0x80..=0xff => MAC_ROMAN_HIGH[slot - 0x80],
                _ => latin(code),
            },
            _ => return None,
        };
    }
    Some(table)
}

/// A predefined base encoding: its characters and, where known, glyph names.
struct Base {
    chars: CharTable,
    names: Option<Names>,
    win_ansi: bool,
}

fn base_of(pdf: &lopdf::Document, name: &[u8]) -> Option<Base> {
    Some(Base {
        chars: base_table(pdf, name)?,
        names: base_names(name),
        win_ansi: name == b"WinAnsiEncoding",
    })
}

/// Glyphs a viewer may draw for `codes`: the built-in encoding, `/Differences`
/// names, and the base encoding by glyph name and by character.
fn used_glyphs(
    face: &Face,
    codes: impl IntoIterator<Item = u8>,
    base: Option<&Base>,
    standard: &CharTable,
    differences: &[(u8, Vec<u8>)],
) -> BTreeSet<u16> {
    let mut glyphs = BTreeSet::new();
    for code in codes {
        let slot = usize::from(code);
        // Every route a viewer might take from the code to a glyph is kept.
        glyphs.extend(face.builtin[slot]);
        let named = differences.iter().rev().find(|(c, _)| *c == code);
        if let Some((_, name)) = named {
            glyphs.extend(glyphs_matching(face, Some(name), None));
            continue;
        }
        let mut chars = Vec::new();
        let mut name = match base {
            Some(base) => {
                chars.extend(base.chars[slot]);
                base.names.map(|names| names[slot])
            }
            None => {
                chars.extend(standard[slot]);
                Some(PredefinedEncoding::Standard.name(code))
            }
        };
        if base.is_some_and(|base| base.win_ansi) && code > 40 && chars.is_empty() {
            chars.push('\u{2022}');
            name = Some("bullet");
        }
        if let Some(name) = name.filter(|n| !n.is_empty() && *n != ".notdef") {
            glyphs.extend(glyphs_matching(face, Some(name.as_bytes()), None));
        }
        for ch in chars {
            glyphs.extend(glyphs_for_char(face, ch));
        }
    }
    glyphs
}

// Fields in an AcroForm default resource dictionary can be typed into later.
fn form_default_font(pdf: &lopdf::Document, font_id: ObjectId, limit: u16) -> bool {
    pdf.objects.values().any(|object| {
        let Ok(owner) = object.as_dict() else {
            return false;
        };
        let Ok(resources) = owner.get(b"DR") else {
            return false;
        };
        let Some(resources) = dict(pdf, resources, limit) else {
            return true;
        };
        let Ok(fonts) = resources.get(b"Font") else {
            return false;
        };
        match dict(pdf, fonts, limit) {
            Some(fonts) => fonts
                .iter()
                .any(|(_, v)| v.as_reference().ok() == Some(font_id)),
            None => true,
        }
    })
}

/// Prune unused glyph outlines from a simple Type1C font without renumbering anything.
pub fn subset_simple_cff(
    doc: &mut PdfDoc,
    font_id: ObjectId,
    usage: &GlyphUsage,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<SubsetOutcome, FontError> {
    let depth = limits.max_nesting_depth;
    if usage.incomplete {
        return Ok(SubsetOutcome::Skipped("ambiguous glyph scan"));
    }
    let Some(codes) = usage.simple.get(&font_id) else {
        return Ok(SubsetOutcome::Skipped("font not used"));
    };
    let pdf = doc.inner();
    let Some(font) = pdf.get_object(font_id).ok().and_then(|o| o.as_dict().ok()) else {
        return Ok(SubsetOutcome::Skipped("missing font"));
    };
    if font.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Type1".as_slice()) {
        return Ok(SubsetOutcome::Skipped("font is not Type 1"));
    }
    let Some(descriptor_object) = font.get(b"FontDescriptor").ok() else {
        return Ok(SubsetOutcome::Skipped("missing font descriptor"));
    };
    if shared_font_program(pdf, descriptor_object, b"FontFile3", depth) {
        return Ok(SubsetOutcome::Skipped("shared font program"));
    }
    let Some(mut descriptor) = dict(pdf, descriptor_object, depth).cloned() else {
        return Ok(SubsetOutcome::Skipped("missing font descriptor"));
    };
    let Some(program) = descriptor
        .get(b"FontFile3")
        .ok()
        .and_then(|o| resolve(pdf, o, depth))
        .and_then(|o| o.as_stream().ok())
    else {
        return Ok(SubsetOutcome::Skipped("missing CFF program"));
    };
    if program.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Type1C".as_slice()) {
        return Ok(SubsetOutcome::Skipped("font program is not Type1C"));
    }
    if form_default_font(pdf, font_id, depth) {
        return Ok(SubsetOutcome::Skipped("form default resource"));
    }
    let mut budget = doc.decode_budget();
    let bytes = match content_bytes(program, &mut budget) {
        Ok(bytes) => bytes,
        Err(FontError::Malformed) => return Ok(SubsetOutcome::Skipped("unsupported font stream")),
        Err(error) => return Err(error),
    };
    let Some(face) = parse(&bytes) else {
        return Ok(SubsetOutcome::Skipped("unsupported CFF program"));
    };
    // Base encoding, then differences; a missing base means the built-in one
    // for embedded programs, with Standard as the non-symbolic fallback.
    let encoding = font
        .get(b"Encoding")
        .ok()
        .and_then(|o| resolve(pdf, o, depth));
    let mut base: Option<Base> = None;
    let mut differences: Vec<(u8, Vec<u8>)> = Vec::new();
    match encoding {
        None => {}
        Some(Object::Name(name)) => match base_of(pdf, name) {
            Some(found) => base = Some(found),
            None => return Ok(SubsetOutcome::Skipped("unsupported font encoding")),
        },
        Some(Object::Dictionary(encoding)) => {
            match encoding
                .get(b"BaseEncoding")
                .ok()
                .and_then(|o| resolve(pdf, o, depth))
            {
                None => {}
                Some(Object::Name(name)) => match base_of(pdf, name) {
                    Some(found) => base = Some(found),
                    None => return Ok(SubsetOutcome::Skipped("unsupported font encoding")),
                },
                Some(_) => return Ok(SubsetOutcome::Skipped("unsupported font encoding")),
            }
            if let Ok(items) = encoding.get(b"Differences") {
                let Some(Object::Array(items)) = resolve(pdf, items, depth) else {
                    return Ok(SubsetOutcome::Skipped("invalid differences"));
                };
                let mut next = None;
                for item in items {
                    match item {
                        Object::Integer(n) => next = u8::try_from(*n).ok(),
                        Object::Name(name) => {
                            let Some(code) = next else {
                                return Ok(SubsetOutcome::Skipped("invalid differences"));
                            };
                            differences.push((code, name.clone()));
                            next = code.checked_add(1);
                        }
                        _ => return Ok(SubsetOutcome::Skipped("invalid differences")),
                    }
                }
            }
        }
        Some(_) => return Ok(SubsetOutcome::Skipped("unsupported font encoding")),
    }
    let standard = base_table(pdf, b"StandardEncoding").unwrap_or([None; 256]);
    let glyphs = used_glyphs(
        &face,
        codes.iter().copied(),
        base.as_ref(),
        &standard,
        &differences,
    );
    if glyphs.iter().chain([&0]).collect::<BTreeSet<_>>().len() >= face.names.len() {
        return Ok(SubsetOutcome::Skipped("no unused glyphs"));
    }
    let Some(subset) = prune(&bytes, &glyphs, ctx)? else {
        return Ok(SubsetOutcome::Skipped("unsupported CFF program"));
    };
    let baseline = if program.dict.has(b"Filter") {
        Some(program.content.len())
    } else {
        flate_len(&bytes)
    };
    if flate_len(&subset)
        .zip(baseline)
        .is_none_or(|(new, old)| new >= old)
    {
        return Ok(SubsetOutcome::Skipped("no size gain"));
    }
    let kept = glyphs.len();
    // Keep entries such as /Metadata; the new data is plain, so drop the filter chain.
    let mut program_dict = program.dict.clone();
    for key in [&b"Filter"[..], b"DecodeParms", b"DL"] {
        program_dict.remove(key);
    }
    program_dict.set("Subtype", "Type1C");
    let pdf = doc.inner_mut()?;
    let file_id = pdf.add_object(Stream::new(program_dict, subset));
    descriptor.set("FontFile3", file_id);
    descriptor.remove(b"CharSet");
    let descriptor_id = pdf.add_object(descriptor);
    pdf.get_object_mut(font_id)
        .and_then(Object::as_dict_mut)
        .map_err(|_| FontError::Malformed)?
        .set("FontDescriptor", descriptor_id);
    Ok(SubsetOutcome::Subset { glyphs: kept })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prune(bytes: &[u8], keep: &BTreeSet<u16>) -> Option<Vec<u8>> {
        super::prune(bytes, keep, &ToolCtx::default()).unwrap()
    }

    /// Printable ASCII glyph names use the CFF standard string ids.
    fn sid(c: char) -> u16 {
        c as u16 - 31
    }

    fn index(elements: &[Vec<u8>]) -> Vec<u8> {
        build_index(&elements.iter().map(Vec::as_slice).collect::<Vec<_>>()).unwrap()
    }

    struct Spec {
        /// SID per glyph, starting at glyph 1.
        sids: Vec<u16>,
        strings: Vec<&'static str>,
        /// Charstring per glyph, including `.notdef`.
        charstrings: Vec<Vec<u8>>,
        global: Vec<Vec<u8>>,
        local: Vec<Vec<u8>>,
        /// Custom encoding: code for glyph 1, 2, ...; empty selects Standard.
        codes: Vec<u8>,
        /// Extra raw Top DICT entries.
        extra_top: Vec<u8>,
        /// Put a Private DICT without Subrs before the CharStrings INDEX.
        private_first: bool,
    }

    /// Lay out header, Name, Top DICT, String, Global Subr, charset, Encoding,
    /// CharStrings, Private and Local Subrs in the usual order.
    fn build(spec: &Spec) -> Vec<u8> {
        let names = index(&[b"T".to_vec()]);
        let strings = index(
            &spec
                .strings
                .iter()
                .map(|s| s.as_bytes().to_vec())
                .collect::<Vec<_>>(),
        );
        let global = index(&spec.global);
        let mut charset = vec![0];
        for s in &spec.sids {
            charset.extend_from_slice(&s.to_be_bytes());
        }
        let encoding = if spec.codes.is_empty() {
            Vec::new()
        } else {
            let mut e = vec![0, spec.codes.len() as u8];
            e.extend_from_slice(&spec.codes);
            e
        };
        let charstrings = index(&spec.charstrings);
        let local = if spec.private_first {
            Vec::new()
        } else {
            index(&spec.local)
        };
        let private_len = 6;
        let int = |v: usize| {
            let mut o = vec![29];
            o.extend_from_slice(&(v as i32).to_be_bytes());
            o
        };
        // Either a Private DICT with Subrs, or one with a width and no Subrs.
        let mut private = int(if spec.private_first { 500 } else { private_len });
        private.push(if spec.private_first { 20 } else { 19 });
        assert_eq!(private.len(), private_len);
        let top_len = 2 + 1 + 2 + 29 + spec.extra_top.len();
        let mut at = 4 + names.len() + top_len + strings.len() + global.len();
        let charset_at = at;
        at += charset.len();
        let encoding_at = if spec.codes.is_empty() { 0 } else { at };
        at += encoding.len();
        if spec.private_first {
            at += private.len();
        }
        let charstrings_at = at;
        at += charstrings.len();
        let private_at = if spec.private_first {
            charstrings_at - private.len()
        } else {
            at
        };
        let mut dict = Vec::new();
        dict.extend(int(charset_at));
        dict.push(15);
        dict.extend(int(encoding_at));
        dict.push(16);
        dict.extend(int(charstrings_at));
        dict.push(17);
        dict.extend(int(private_len));
        dict.extend(int(private_at));
        dict.push(18);
        assert_eq!(dict.len(), 29);
        dict.extend_from_slice(&spec.extra_top);
        let mut out = vec![1, 0, 4, 4];
        out.extend(names);
        out.extend(index(&[dict]));
        out.extend(strings);
        out.extend(global);
        out.extend(charset);
        out.extend(encoding);
        if spec.private_first {
            out.extend(&private);
        }
        out.extend(charstrings);
        if !spec.private_first {
            out.extend(private);
            out.extend(local);
        }
        out
    }

    const NOTDEF: [u8; 1] = [14];
    /// `10 20 rmoveto`, a few `rlineto` arguments derived from `seed`, `endchar`.
    fn outline(seed: u8) -> Vec<u8> {
        vec![149, 159, 21, 169, 179, seed.max(40), seed.max(40), 5, 14]
    }
    /// `10 20 rmoveto` then a `callsubr 0`, which draws and returns.
    fn calls_local() -> Vec<u8> {
        vec![149, 159, 21, 32, 10, 14]
    }

    fn spec(glyphs: &str) -> Spec {
        Spec {
            sids: glyphs.chars().map(sid).collect(),
            strings: Vec::new(),
            charstrings: std::iter::once(NOTDEF.to_vec())
                .chain(glyphs.bytes().map(outline))
                .collect(),
            global: vec![vec![149, 159, 21, 11]],
            local: vec![vec![169, 179, 5, 11]],
            codes: Vec::new(),
            extra_top: Vec::new(),
            private_first: false,
        }
    }

    fn private_at(bytes: &[u8]) -> usize {
        let names = parse_index(bytes, usize::from(bytes[2])).unwrap();
        let tops = parse_index(bytes, names.end).unwrap();
        let entries = parse_dict(element(bytes, &tops, 0).unwrap()).unwrap();
        let private = entries.iter().find(|e| e.operator == 18).unwrap();
        usize::try_from(private.operands[1].1.unwrap()).unwrap()
    }

    fn charstring(bytes: &[u8], gid: usize) -> Vec<u8> {
        let layout = layout(bytes).unwrap();
        element(bytes, &layout.cs, gid).unwrap().to_vec()
    }

    #[test]
    fn synthetic_program_parses_as_built() {
        let bytes = build(&spec("ABCDE"));
        let face = parse(&bytes).unwrap();
        assert_eq!(face.names[1], b"A");
        assert_eq!(face.names[5], b"E");
        assert!(face.predefined_encoding);
        assert_eq!(face.builtin[usize::from(b'C')], Some(3));
    }

    #[test]
    fn unused_glyphs_become_lone_endchar_and_layout_stays_valid() {
        let bytes = build(&spec("ABCDE"));
        let out = prune(&bytes, &BTreeSet::from([1, 3])).unwrap();
        assert!(out.len() < bytes.len());
        assert_eq!(parse(&bytes), parse(&out));
        for gid in [0, 2, 4, 5] {
            assert_eq!(charstring(&out, gid), [ENDCHAR], "glyph {gid}");
        }
        for gid in [1, 3] {
            assert_eq!(
                charstring(&out, gid),
                charstring(&bytes, gid),
                "glyph {gid}"
            );
        }
        // Subroutines, charset, encoding and the Private DICT survive byte for byte.
        let (a, b) = (layout(&bytes).unwrap(), layout(&out).unwrap());
        assert_eq!(a.private, b.private);
        assert_eq!(&bytes[..a.top_start], &out[..b.top_start]);
        assert_eq!(&bytes[a.top_end..a.cs_off], &out[b.top_end..b.cs_off]);
        assert_eq!(&bytes[a.cs.end..], &out[b.cs.end..]);
    }

    #[test]
    fn custom_encoding_and_custom_strings_survive() {
        let mut s = spec("ABC");
        s.sids.push(391);
        s.strings.push("Aacute");
        s.charstrings.push(outline(7));
        s.codes = vec![65, 66, 67, 193];
        let bytes = build(&s);
        let out = prune(&bytes, &BTreeSet::from([4])).unwrap();
        let (a, b) = (parse(&bytes).unwrap(), parse(&out).unwrap());
        assert_eq!(b.names[4], b"Aacute");
        assert_eq!(a, b);
        assert_eq!(b.builtin[193], Some(4));
        assert!(!b.predefined_encoding);
    }

    #[test]
    fn accent_components_of_a_kept_composite_are_kept() {
        let acute = PredefinedEncoding::Standard.sid(194).unwrap().to_u16();
        let mut s = spec("AB");
        s.sids.extend([acute, 391]);
        s.strings.push("Aacute");
        s.charstrings.push(outline(1));
        // adx ady bchar achar endchar with bchar 'A' and achar 'acute'.
        s.charstrings.push(vec![139, 139, 204, 247, 86, 14]);
        let bytes = build(&s);
        let out = prune(&bytes, &BTreeSet::from([4])).unwrap();
        assert_eq!(charstring(&out, 1), charstring(&bytes, 1), "base kept");
        assert_eq!(charstring(&out, 3), charstring(&bytes, 3), "accent kept");
        assert_eq!(charstring(&out, 4), charstring(&bytes, 4));
        assert_eq!(charstring(&out, 2), [ENDCHAR], "unrelated glyph pruned");
    }

    #[test]
    fn accent_composite_hidden_in_a_subroutine_is_found() {
        let acute = PredefinedEncoding::Standard.sid(194).unwrap().to_u16();
        let mut s = spec("AB");
        s.sids.extend([acute, 391]);
        s.strings.push("Aacute");
        s.charstrings.push(outline(1));
        s.charstrings.push(calls_local());
        s.local = vec![vec![139, 139, 204, 247, 86, 14]];
        let bytes = build(&s);
        let out = prune(&bytes, &BTreeSet::from([4])).unwrap();
        assert_eq!(
            charstring(&out, 1),
            charstring(&bytes, 1),
            "base kept via subr"
        );
        assert_eq!(
            charstring(&out, 3),
            charstring(&bytes, 3),
            "accent kept via subr"
        );
        assert_eq!(charstring(&out, 2), [ENDCHAR], "unrelated glyph pruned");
    }

    #[test]
    fn hintmask_bytes_do_not_look_like_operators() {
        let acute = PredefinedEncoding::Standard.sid(194).unwrap().to_u16();
        let mut s = spec("AB");
        s.sids.extend([acute, 391]);
        s.strings.push("Aacute");
        s.charstrings.push(outline(1));
        // Nine stems need two mask bytes; the second one equals `endchar`.
        let mut composite: Vec<u8> = (0..18).map(|i| 139 + i).collect();
        composite.extend([18, 19, 0xff, 14, 139, 139, 204, 247, 86, 14]);
        s.charstrings.push(composite);
        let bytes = build(&s);
        let out = prune(&bytes, &BTreeSet::from([4])).unwrap();
        assert_eq!(charstring(&out, 1), charstring(&bytes, 1));
        assert_eq!(charstring(&out, 3), charstring(&bytes, 3));
    }

    #[test]
    fn missing_accent_glyph_refuses_the_font() {
        let mut s = spec("A");
        s.sids.push(391);
        s.strings.push("Aacute");
        s.charstrings.push(vec![139, 139, 204, 247, 86, 14]);
        assert!(prune(&build(&s), &BTreeSet::from([2])).is_none());
    }

    #[test]
    fn programs_that_cannot_be_walked_are_refused() {
        // Reserved operator, arithmetic, bad subroutine, runaway recursion.
        for bad in [
            vec![149, 2, 14],
            vec![139, 139, 12, 10, 14],
            vec![37, 29, 14],
            vec![10, 14],
            vec![32, 10, 14],
        ] {
            let mut s = spec("AB");
            s.charstrings[2] = bad;
            s.local = vec![vec![32, 10]];
            assert!(prune(&build(&s), &BTreeSet::from([2])).is_none());
        }
    }

    /// Glyphs call a mid-level subroutine, which calls a large leaf many
    /// times: each glyph interprets megabytes from a few kilobytes of program.
    fn amplifying(glyphs: usize) -> Spec {
        let mut s = spec(&"A".repeat(glyphs));
        // Distinct names are not needed: only the charstrings are walked.
        let leaf: Vec<u8> = [139_u8, 22].repeat(2000).into_iter().chain([11]).collect();
        let mid: Vec<u8> = [32_u8, 10].repeat(100).into_iter().chain([11]).collect();
        // Local subr 0 is the leaf, local subr 1 the mid-level routine.
        s.local = vec![leaf, mid];
        let call_mid: Vec<u8> = [33_u8, 10].repeat(40).into_iter().chain([14]).collect();
        for glyph in s.charstrings.iter_mut().skip(1) {
            *glyph = call_mid.clone();
        }
        s
    }

    #[test]
    fn amplifying_subroutines_are_refused_quickly() {
        let bytes = build(&amplifying(12));
        let keep: BTreeSet<u16> = (1..=6).collect();
        let started = std::time::Instant::now();
        assert!(prune(&bytes, &keep).is_none());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn cancellation_stops_the_interpreter() {
        let bytes = build(&amplifying(12));
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let ctx = ToolCtx::with_cancel(cancel);
        let keep: BTreeSet<u16> = (1..=6).collect();
        assert!(super::prune(&bytes, &keep, &ctx).is_err());
    }

    #[test]
    fn seac_rounds_interpret_each_glyph_once() {
        // A chain of composites: each round only walks the glyph it added.
        let acute = PredefinedEncoding::Standard.sid(194).unwrap().to_u16();
        let mut s = spec("AB");
        s.sids.extend([acute, 391]);
        s.strings.push("Aacute");
        s.charstrings.push(outline(1));
        s.charstrings.push(vec![139, 139, 204, 247, 86, 14]);
        let bytes = build(&s);
        assert!(prune(&bytes, &BTreeSet::from([4])).is_some());
    }

    #[test]
    fn modest_subroutine_reuse_still_prunes() {
        let mut s = amplifying(12);
        // Every glyph calls the 4 KB leaf three times.
        let call_leaf: Vec<u8> = [32_u8, 10].repeat(3).into_iter().chain([14]).collect();
        for glyph in s.charstrings.iter_mut().skip(1) {
            *glyph = call_leaf.clone();
        }
        assert!(prune(&build(&s), &(1..=6).collect()).is_some());
    }

    #[test]
    fn private_dict_before_the_charstrings_without_subrs_is_pruned() {
        let mut s = spec("ABCDE");
        s.private_first = true;
        // No glyph may call a local subroutine in this layout.
        for glyph in s.charstrings.iter_mut().skip(1) {
            *glyph = vec![149, 159, 21, 14];
        }
        let bytes = build(&s);
        let before = layout(&bytes).unwrap();
        assert!(before.private.is_some());
        assert!(private_at(&bytes) < before.cs_off, "private sits first");
        let out = prune(&bytes, &BTreeSet::from([1, 3])).unwrap();
        assert!(out.len() < bytes.len());
        assert_eq!(parse(&bytes), parse(&out));
        let after = layout(&out).unwrap();
        assert_eq!(before.private, after.private);
        assert_eq!(charstring(&out, 2), [ENDCHAR]);
        assert_eq!(charstring(&out, 3), charstring(&bytes, 3));
    }

    #[test]
    fn private_dict_and_subrs_on_opposite_sides_are_refused() {
        // Private before the CharStrings, its Subrs after them: the distance
        // between the two would change, so the font stays untouched.
        let mut s = spec("ABCDE");
        s.private_first = true;
        for glyph in s.charstrings.iter_mut().skip(1) {
            *glyph = vec![149, 159, 21, 14];
        }
        let mut bytes = build(&s);
        let at = private_at(&bytes);
        let distance = i32::try_from(layout(&bytes).unwrap().cs.end - at).unwrap();
        bytes[at + 1..at + 5].copy_from_slice(&distance.to_be_bytes());
        bytes[at + 5] = 19;
        bytes.extend(index(&[vec![11]]));
        assert!(prune(&bytes, &BTreeSet::from([1])).is_none());
    }

    fn named_font(extra: &[&'static str]) -> Face {
        let mut s = spec("AB");
        for (i, name) in extra.iter().enumerate() {
            s.sids.push(391 + i as u16);
            s.strings.push(name);
            s.charstrings.push(outline(1));
        }
        parse(&build(&s)).unwrap()
    }

    fn base(name: &[u8]) -> Base {
        base_of(&lopdf::Document::new(), name).unwrap()
    }

    fn standard() -> CharTable {
        base_table(&lopdf::Document::new(), b"StandardEncoding").unwrap()
    }

    #[test]
    fn base_encodings_match_glyphs_outside_the_adobe_glyph_list_by_name() {
        // MacRoman 0xF0 is the Apple logo, named `apple`, which has no AGL entry.
        let face = named_font(&["apple"]);
        // The base carries no character table, so only the name can find it.
        let mac = Base {
            chars: [None; 256],
            names: base_names(b"MacRomanEncoding"),
            win_ansi: false,
        };
        let found = used_glyphs(&face, [0xf0], Some(&mac), &standard(), &[]);
        assert!(found.contains(&3), "{found:?}");
        // The same code under WinAnsi is an undefined bullet, not the logo.
        let win = base(b"WinAnsiEncoding");
        assert!(!used_glyphs(&face, [0xf0], Some(&win), &standard(), &[]).contains(&3));
    }

    #[test]
    fn base_encoding_tables_follow_the_pdf_specification() {
        let mac = base_names(b"MacRomanEncoding").unwrap();
        let win = base_names(b"WinAnsiEncoding").unwrap();
        let std = base_names(b"StandardEncoding").unwrap();
        assert_eq!(
            (mac[0xf0], mac[0xca], mac[0xdb]),
            ("apple", "space", "currency")
        );
        assert_eq!(
            (mac[0x27], mac[0x60], mac[0x41]),
            ("quotesingle", "grave", "A")
        );
        assert_eq!(
            (win[0x27], win[0x60], win[0x80]),
            ("quotesingle", "grave", "Euro")
        );
        assert_eq!(
            (win[0x95], win[0xa0], win[0xad]),
            ("bullet", "space", "hyphen")
        );
        assert_eq!(
            (win[0xe9], win[0xb5], win[0x9f]),
            ("eacute", "mu", "Ydieresis")
        );
        assert_eq!(
            (std[0x27], std[0x60], std[0xae]),
            ("quoteright", "quoteleft", "fi")
        );
        assert!(base_names(b"MacExpertEncoding").is_none());
    }

    #[test]
    fn base_encoding_character_matching_still_works() {
        // `uni00E9` is not named `eacute`, but it is the same character.
        let face = named_font(&["uni00E9"]);
        let win = base(b"WinAnsiEncoding");
        assert!(used_glyphs(&face, [0xe9], Some(&win), &standard(), &[]).contains(&3));
        let none = used_glyphs(&face, [65], None, &standard(), &[]);
        assert!(none.contains(&1) && !none.contains(&3));
    }

    #[test]
    fn glyph_name_tables_agree_with_the_character_tables() {
        // Every named code must decode to the character of its glyph name, so the
        // name path only adds exactness and never contradicts the character path.
        for encoding in [
            &b"MacRomanEncoding"[..],
            b"WinAnsiEncoding",
            b"StandardEncoding",
        ] {
            let base = base(encoding);
            let names = base.names.unwrap();
            for (code, name) in names.into_iter().enumerate() {
                if name.is_empty() || name == ".notdef" {
                    continue;
                }
                assert_eq!(
                    char_of(name.as_bytes()),
                    base.chars[code],
                    "{} {code:#x} {name}",
                    String::from_utf8_lossy(encoding)
                );
            }
        }
    }

    #[test]
    fn nothing_to_prune_returns_none() {
        let bytes = build(&spec("AB"));
        assert!(prune(&bytes, &BTreeSet::from([1, 2])).is_none());
    }

    #[test]
    fn damaged_programs_are_refused() {
        let bytes = build(&spec("ABCDE"));
        assert!(prune(&bytes[..bytes.len() / 2], &BTreeSet::from([1])).is_none());
        assert!(prune(b"", &BTreeSet::new()).is_none());
        assert!(prune(&[1, 0, 4, 4, 0xff, 0xff], &BTreeSet::new()).is_none());
        for cut in 0..bytes.len() {
            // Never panics and never returns a program that fails its own check.
            if let Some(out) = prune(&bytes[..cut], &BTreeSet::from([1])) {
                assert!(parse(&out).is_some());
            }
        }
        // A Private DICT offset that points into the CharStrings INDEX.
        let mut broken = bytes.clone();
        let layout = layout(&bytes).unwrap();
        let operand = layout.top_start + 5 + 24;
        let inside = i32::try_from(layout.cs_off + 4).unwrap();
        broken[operand..operand + 4].copy_from_slice(&inside.to_be_bytes());
        assert!(prune(&broken, &BTreeSet::from([1])).is_none());
    }

    #[test]
    fn single_byte_corruption_never_panics_or_yields_a_broken_program() {
        let mut s = spec("ABCDE");
        s.codes = vec![65, 66, 67, 68, 69];
        let bytes = build(&s);
        let keep = BTreeSet::from([2, 4]);
        let original = prune(&bytes, &keep).unwrap();
        assert!(original.len() < bytes.len());
        for at in 0..bytes.len() {
            for value in [0, 1, 0x0e, 0x7f, 0xff, bytes[at].wrapping_add(1)] {
                let mut mutated = bytes.clone();
                mutated[at] = value;
                let Some(out) = prune(&mutated, &keep) else {
                    continue;
                };
                // Whatever was accepted keeps the glyph table and shrinks.
                assert!(out.len() < mutated.len(), "byte {at}={value:#x}");
                assert_eq!(parse(&mutated), parse(&out), "byte {at}={value:#x}");
                assert!(layout(&out).is_some(), "byte {at}={value:#x}");
            }
        }
    }

    #[test]
    fn unsupported_top_dict_operators_are_refused() {
        assert!(prune(&build(&spec("ABCDE")), &BTreeSet::from([1])).is_some());
        // SyntheticBase, ROS and a Type 1 charstring type.
        for extra in [
            vec![139, 12, 20],
            vec![139, 139, 139, 12, 30],
            vec![140, 12, 6],
        ] {
            let mut s = spec("ABCDE");
            s.extra_top = extra;
            assert!(prune(&build(&s), &BTreeSet::from([1])).is_none());
        }
        // An explicit Type 2 charstring type is fine.
        let mut s = spec("ABCDE");
        s.extra_top = vec![141, 12, 6];
        assert!(prune(&build(&s), &BTreeSet::from([1])).is_some());
    }

    #[test]
    fn glyph_lookup_follows_names_and_unicode_aliases() {
        let mut s = spec("AB");
        s.sids.extend([391, 392]);
        s.strings.extend(["uni00A0", "A.sc"]);
        s.charstrings.extend([outline(1), outline(2)]);
        let face = parse(&build(&s)).unwrap();
        assert_eq!(glyphs_for_char(&face, 'A'), BTreeSet::from([1, 4]));
        assert_eq!(
            glyphs_matching(&face, Some(b"B"), None),
            BTreeSet::from([2])
        );
        assert!(glyphs_matching(&face, Some(b"missing"), None).is_empty());
        // U+00A0 reaches the glyph with that Unicode alias and folds onto `space`.
        assert_eq!(glyphs_for_char(&face, '\u{a0}'), BTreeSet::from([3]));
    }

    #[test]
    fn index_builder_round_trips_every_offset_size() {
        for len in [0_usize, 1, 200, 70_000] {
            let element = vec![7_u8; len];
            let bytes = build_index(&[&element, &element]).unwrap();
            let parsed = parse_index(&bytes, 0).unwrap();
            assert_eq!(parsed.count, 2);
            assert_eq!(element_at(&bytes, &parsed, 1), element);
            assert_eq!(parsed.end, bytes.len());
        }
    }

    fn element_at(data: &[u8], index: &Index, i: usize) -> Vec<u8> {
        element(data, index, i).unwrap().to_vec()
    }
}

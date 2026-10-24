//! Content-stream interpreter.
//!
//! Tracks the graphics and text state of one page, decodes the glyphs of every text-showing
//! operator into positioned Unicode text, and counts painted images. Form XObjects run on an
//! explicit frame stack, so nesting never recurses.

use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use super::{
    ReadError,
    encoding::Encodings,
    font::{Font, normalize_into},
    lexer::{Token, next_token, skip_array, skip_dict, skip_inline_image},
    objects::{Budget, get_array, get_dict, get_name, number, resolve},
};

/// Most content-stream operators interpreted for one page, including its form XObjects.
/// Interpretation stops at the cap and the reader reports the page as truncated.
pub const MAX_PAGE_OPERATORS: u64 = 4_000_000;

/// Most glyphs collected for one page.
pub const MAX_PAGE_GLYPHS: usize = 1_000_000;

/// Deepest chain of nested form XObjects followed.
pub const MAX_FORM_DEPTH: usize = 12;

/// Deepest graphics-state stack (`q` without `Q`) tracked.
const MAX_GSTATE_DEPTH: usize = 256;

/// Most operands kept for one operator.
const MAX_OPERANDS: usize = 128;

/// Most elements kept in one array operand.
const MAX_ARRAY_ITEMS: usize = 65_536;

/// Deepest array nesting kept in one operand.
const MAX_ARRAY_DEPTH: usize = 16;

/// Most font-name lookups remembered per content stream.
const MAX_FONT_NAMES: usize = 1_024;

/// Glyph flags.
pub(super) const SPACE: u8 = 1;
/// The glyph is the first one shown after a text-positioning operator.
pub(super) const CHUNK_START: u8 = 2;
/// The glyph is filled and stroked, which producers use to embolden text.
pub(super) const STROKED: u8 = 4;

/// An affine transform `[a b c d e f]` mapping `(x, y)` to `(ax + cy + e, bx + dy + f)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Matrix(pub(super) [f64; 6]);

impl Matrix {
    pub(super) const IDENTITY: Self = Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    /// The transform that applies `self` first, then `other`.
    pub(super) fn then(&self, other: &Self) -> Self {
        let [a, b, c, d, e, f] = self.0;
        let [oa, ob, oc, od, oe, of] = other.0;
        Self([
            a * oa + b * oc,
            a * ob + b * od,
            c * oa + d * oc,
            c * ob + d * od,
            e * oa + f * oc + oe,
            e * ob + f * od + of,
        ])
    }

    pub(super) fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }

    fn apply_vector(&self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, _, _] = self.0;
        (a * x + c * y, b * x + d * y)
    }

    fn translate(&mut self, tx: f64, ty: f64) {
        let [a, b, c, d, e, f] = self.0;
        self.0[4] = tx * a + ty * c + e;
        self.0[5] = tx * b + ty * d + f;
    }

    fn from_operands(values: [f64; 6]) -> Self {
        Self(values)
    }
}

/// One decoded glyph in page display space (points, origin top-left, y down).
#[derive(Clone, Copy, Debug)]
pub(super) struct Glyph {
    /// Origin on the baseline.
    pub(super) x: f64,
    pub(super) y: f64,
    /// Unit vector of the writing direction.
    pub(super) ux: f64,
    pub(super) uy: f64,
    /// Advance width along the writing direction.
    pub(super) width: f64,
    /// Rendered font size.
    pub(super) size: f64,
    /// Byte range of the glyph's text in [`PageContent::text`].
    pub(super) text_start: u32,
    pub(super) text_end: u32,
    /// Index into [`DocState::styles`].
    pub(super) font: u32,
    pub(super) flags: u8,
}

/// Bold and italic flags of a loaded font.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct FontStyle {
    pub(super) bold: bool,
    pub(super) italic: bool,
}

/// What one page paints.
#[derive(Debug, Default)]
pub(super) struct PageContent {
    pub(super) glyphs: Vec<Glyph>,
    pub(super) text: String,
    pub(super) images: u32,
    pub(super) unmapped: u64,
    pub(super) truncated: bool,
}

impl PageContent {
    pub(super) fn glyph_text(&self, glyph: &Glyph) -> &str {
        self.text
            .get(glyph.text_start as usize..glyph.text_end as usize)
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum FontKey {
    Reference(ObjectId),
    /// A direct font dictionary, keyed by its address inside the loaded document.
    Direct(usize),
}

/// State shared by all pages of a document.
pub(super) struct DocState<'d> {
    pub(super) doc: &'d Document,
    pub(super) budget: Budget,
    encodings: Encodings,
    font_keys: HashMap<FontKey, u32>,
    fonts: Vec<Rc<Font>>,
    pub(super) styles: Vec<FontStyle>,
    /// Streams decoded once; the second use caches them.
    seen_streams: HashSet<ObjectId>,
    cached_streams: HashMap<ObjectId, Rc<[u8]>>,
}

impl<'d> DocState<'d> {
    pub(super) fn new(doc: &'d Document, budget: Budget) -> Self {
        Self {
            doc,
            budget,
            encodings: Encodings::default(),
            font_keys: HashMap::new(),
            fonts: Vec::new(),
            styles: Vec::new(),
            seen_streams: HashSet::new(),
            cached_streams: HashMap::new(),
        }
    }

    /// Decoded bytes of a stream object. Every use is charged against the budget, and streams
    /// used more than once are decoded at most twice.
    fn stream_bytes(
        &mut self,
        id: Option<ObjectId>,
        stream: &Stream,
    ) -> Result<Option<Rc<[u8]>>, ReadError> {
        if let Some(id) = id
            && let Some(data) = self.cached_streams.get(&id)
        {
            self.budget.charge(data.len())?;
            return Ok(Some(Rc::clone(data)));
        }
        let Some(data) = self.budget.decode(stream)? else {
            return Ok(None);
        };
        let data: Rc<[u8]> = Rc::from(data);
        if let Some(id) = id
            && !self.seen_streams.insert(id)
        {
            self.cached_streams.insert(id, Rc::clone(&data));
        }
        Ok(Some(data))
    }

    fn font_index(&mut self, object: &'d Object) -> Result<u32, ReadError> {
        let dict = resolve(self.doc, object).and_then(|object| object.as_dict().ok());
        let key = match (object.as_reference(), dict) {
            (Ok(id), _) => FontKey::Reference(id),
            (Err(_), Some(dict)) => FontKey::Direct(std::ptr::from_ref(dict) as usize),
            (Err(_), None) => FontKey::Direct(0),
        };
        if let Some(&index) = self.font_keys.get(&key) {
            return Ok(index);
        }
        let font = match dict {
            Some(dict) => Font::load(self.doc, dict, &mut self.budget, &mut self.encodings)?,
            None => Font::unknown(),
        };
        self.budget.charge_work(font.work_bytes())?;
        let index = u32::try_from(self.fonts.len()).unwrap_or(u32::MAX);
        self.styles.push(FontStyle {
            bold: font.bold,
            italic: font.italic,
        });
        self.fonts.push(Rc::new(font));
        self.font_keys.insert(key, index);
        Ok(index)
    }
}

#[derive(Clone, Debug)]
struct GState {
    ctm: Matrix,
    font: Option<u32>,
    font_size: f64,
    char_spacing: f64,
    word_spacing: f64,
    h_scale: f64,
    leading: f64,
    rise: f64,
    render_mode: i64,
}

impl GState {
    const fn new(ctm: Matrix) -> Self {
        Self {
            ctm,
            font: None,
            font_size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            h_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
            render_mode: 0,
        }
    }
}

struct Frame<'d> {
    data: Rc<[u8]>,
    pos: usize,
    resources: Option<&'d Dictionary>,
    /// Graphics-state stack depth below which this frame's `Q` operators cannot pop.
    floor: usize,
    /// `q` operators past [`MAX_GSTATE_DEPTH`] still waiting for their `Q`.
    overflow: usize,
    form: Option<ObjectId>,
    fonts: HashMap<Vec<u8>, Option<u32>>,
}

#[derive(Clone, Debug)]
enum Operand {
    Number(f64),
    Name(Vec<u8>),
    String(Vec<u8>),
    Array(Vec<Operand>),
    Other,
}

impl Operand {
    const fn number(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }
}

/// Interprets a page's content streams.
pub(super) fn interpret_page<'d>(
    state: &mut DocState<'d>,
    contents: &[ObjectId],
    resources: Option<&'d Dictionary>,
    display: Matrix,
) -> Result<PageContent, ReadError> {
    let mut data: Vec<u8> = Vec::new();
    let mut single: Option<Rc<[u8]>> = None;
    for &id in contents {
        let Some(Object::Stream(stream)) = state.doc.objects.get(&id) else {
            continue;
        };
        let Some(bytes) = state.stream_bytes(Some(id), stream)? else {
            continue;
        };
        if contents.len() == 1 {
            single = Some(bytes);
        } else {
            data.extend_from_slice(&bytes);
            data.push(b'\n');
        }
    }
    let data = single.unwrap_or_else(|| Rc::from(data));
    let mut interpreter = Interpreter {
        state,
        frames: vec![Frame {
            data,
            pos: 0,
            resources,
            floor: 0,
            overflow: 0,
            form: None,
            fonts: HashMap::new(),
        }],
        gstate: GState::new(display),
        saved: Vec::new(),
        tm: Matrix::IDENTITY,
        tlm: Matrix::IDENTITY,
        chunk_start: true,
        operands: Vec::new(),
        out: PageContent::default(),
        operators: 0,
        scratch: String::new(),
    };
    interpreter.run()?;
    Ok(interpreter.out)
}

struct Interpreter<'s, 'd> {
    state: &'s mut DocState<'d>,
    frames: Vec<Frame<'d>>,
    gstate: GState,
    saved: Vec<GState>,
    tm: Matrix,
    tlm: Matrix,
    chunk_start: bool,
    operands: Vec<Operand>,
    out: PageContent,
    operators: u64,
    scratch: String,
}

impl Interpreter<'_, '_> {
    fn run(&mut self) -> Result<(), ReadError> {
        'frames: while let Some(frame) = self.frames.last() {
            let data = Rc::clone(&frame.data);
            let mut pos = frame.pos;
            loop {
                let Some(token) = next_token(&data, &mut pos) else {
                    self.pop_frame();
                    continue 'frames;
                };
                let operator = match token {
                    Token::Keyword(operator) => operator,
                    other => {
                        let operand = read_operand(other, &data, &mut pos);
                        if self.operands.len() >= MAX_OPERANDS {
                            self.operands.clear();
                        }
                        self.operands.push(operand);
                        continue;
                    }
                };
                self.operators += 1;
                if self.operators > MAX_PAGE_OPERATORS {
                    self.out.truncated = true;
                    return Ok(());
                }
                if operator == b"BI" {
                    skip_inline_image(&data, &mut pos);
                    self.out.images += 1;
                    self.operands.clear();
                    continue;
                }
                if let Some(frame) = self.frames.last_mut() {
                    frame.pos = pos;
                }
                let pushed = self.execute(operator)?;
                self.operands.clear();
                if self.out.truncated {
                    return Ok(());
                }
                if pushed {
                    continue 'frames;
                }
            }
        }
        Ok(())
    }

    fn pop_frame(&mut self) {
        let Some(frame) = self.frames.pop() else {
            return;
        };
        if frame.form.is_some() {
            self.saved.truncate(frame.floor);
            if let Some(state) = self.saved.pop() {
                self.gstate = state;
            }
        }
    }

    fn numbers<const N: usize>(&self) -> Option<[f64; N]> {
        let start = self.operands.len().checked_sub(N)?;
        let mut values = [0.0; N];
        for (slot, operand) in values.iter_mut().zip(&self.operands[start..]) {
            *slot = operand.number()?;
        }
        Some(values)
    }

    fn execute(&mut self, operator: &[u8]) -> Result<bool, ReadError> {
        match operator {
            b"q" => self.save(),
            b"Q" => self.restore(),
            b"cm" => {
                if let Some(values) = self.numbers::<6>() {
                    self.gstate.ctm = Matrix::from_operands(values).then(&self.gstate.ctm);
                }
            }
            b"BT" => {
                self.tm = Matrix::IDENTITY;
                self.tlm = Matrix::IDENTITY;
                self.chunk_start = true;
            }
            b"Tf" => {
                if let Some([size]) = self.numbers::<1>()
                    && let Some(Operand::Name(name)) = self.operands.iter().rev().nth(1)
                {
                    let name = name.clone();
                    self.gstate.font = self.select_font(&name)?;
                    self.gstate.font_size = size;
                }
            }
            b"Tc" => {
                if let Some([value]) = self.numbers::<1>() {
                    self.gstate.char_spacing = value;
                }
            }
            b"Tw" => {
                if let Some([value]) = self.numbers::<1>() {
                    self.gstate.word_spacing = value;
                }
            }
            b"Tz" => {
                if let Some([value]) = self.numbers::<1>() {
                    self.gstate.h_scale = value / 100.0;
                }
            }
            b"TL" => {
                if let Some([value]) = self.numbers::<1>() {
                    self.gstate.leading = value;
                }
            }
            b"Ts" => {
                if let Some([value]) = self.numbers::<1>() {
                    self.gstate.rise = value;
                }
            }
            b"Tr" => {
                if let Some([value]) = self.numbers::<1>() {
                    self.gstate.render_mode = value as i64;
                }
            }
            b"Td" => {
                if let Some([tx, ty]) = self.numbers::<2>() {
                    self.move_line(tx, ty);
                }
            }
            b"TD" => {
                if let Some([tx, ty]) = self.numbers::<2>() {
                    self.gstate.leading = -ty;
                    self.move_line(tx, ty);
                }
            }
            b"Tm" => {
                if let Some(values) = self.numbers::<6>() {
                    self.tm = Matrix::from_operands(values);
                    self.tlm = self.tm;
                    self.chunk_start = true;
                }
            }
            b"T*" => self.next_line(),
            b"Tj" => {
                if let Some(Operand::String(bytes)) = self.operands.pop() {
                    self.show(&bytes)?;
                }
            }
            b"'" => {
                self.next_line();
                if let Some(Operand::String(bytes)) = self.operands.pop() {
                    self.show(&bytes)?;
                }
            }
            b"\"" => {
                if let Some(Operand::String(bytes)) = self.operands.pop() {
                    if let Some([word, char_spacing]) = self.numbers::<2>() {
                        self.gstate.word_spacing = word;
                        self.gstate.char_spacing = char_spacing;
                    }
                    self.next_line();
                    self.show(&bytes)?;
                }
            }
            b"TJ" => {
                if let Some(Operand::Array(items)) = self.operands.pop() {
                    for item in items {
                        match item {
                            Operand::String(bytes) => self.show(&bytes)?,
                            Operand::Number(adjust) => {
                                let tx =
                                    -adjust / 1000.0 * self.gstate.font_size * self.gstate.h_scale;
                                self.tm.translate(tx, 0.0);
                            }
                            _ => {}
                        }
                        if self.out.truncated {
                            break;
                        }
                    }
                }
            }
            b"Do" => {
                if let Some(Operand::Name(name)) = self.operands.last() {
                    let name = name.clone();
                    return self.draw_xobject(&name);
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn save(&mut self) {
        if self.saved.len() < MAX_GSTATE_DEPTH {
            self.saved.push(self.gstate.clone());
        } else if let Some(frame) = self.frames.last_mut() {
            frame.overflow += 1;
        }
    }

    fn restore(&mut self) {
        let Some(frame) = self.frames.last_mut() else {
            return;
        };
        if frame.overflow > 0 {
            frame.overflow -= 1;
        } else if self.saved.len() > frame.floor
            && let Some(state) = self.saved.pop()
        {
            self.gstate = state;
        }
    }

    fn move_line(&mut self, tx: f64, ty: f64) {
        self.tlm.translate(tx, ty);
        self.tm = self.tlm;
        self.chunk_start = true;
    }

    fn next_line(&mut self) {
        let leading = self.gstate.leading;
        self.move_line(0.0, -leading);
    }

    fn select_font(&mut self, name: &[u8]) -> Result<Option<u32>, ReadError> {
        let doc = self.state.doc;
        let Some(frame) = self.frames.last() else {
            return Ok(None);
        };
        if let Some(&cached) = frame.fonts.get(name) {
            return Ok(cached);
        }
        let object = frame
            .resources
            .and_then(|resources| get_dict(doc, resources, b"Font"))
            .and_then(|fonts| fonts.get(name).ok());
        let index = match object {
            Some(object) => Some(self.state.font_index(object)?),
            None => None,
        };
        if let Some(frame) = self.frames.last_mut() {
            if frame.fonts.len() >= MAX_FONT_NAMES {
                frame.fonts.clear();
            }
            frame.fonts.insert(name.to_vec(), index);
        }
        Ok(index)
    }

    fn draw_xobject(&mut self, name: &[u8]) -> Result<bool, ReadError> {
        let doc = self.state.doc;
        let Some(frame) = self.frames.last() else {
            return Ok(false);
        };
        let parent_resources = frame.resources;
        let Some(object) = parent_resources
            .and_then(|resources| get_dict(doc, resources, b"XObject"))
            .and_then(|xobjects| xobjects.get(name).ok())
        else {
            return Ok(false);
        };
        let id = object.as_reference().ok();
        let Some(Object::Stream(stream)) = resolve(doc, object) else {
            return Ok(false);
        };
        match get_name(doc, &stream.dict, b"Subtype") {
            Some(b"Image") => {
                self.out.images += 1;
                Ok(false)
            }
            Some(b"Form") => {
                if self.frames.len() > MAX_FORM_DEPTH
                    || self.saved.len() >= MAX_GSTATE_DEPTH
                    || id.is_some_and(|id| self.frames.iter().any(|frame| frame.form == Some(id)))
                {
                    return Ok(false);
                }
                let Some(data) = self.state.stream_bytes(id, stream)? else {
                    return Ok(false);
                };
                let matrix = get_array(doc, &stream.dict, b"Matrix")
                    .and_then(|values| matrix_from_objects(doc, values))
                    .unwrap_or(Matrix::IDENTITY);
                let resources = get_dict(doc, &stream.dict, b"Resources").or(parent_resources);
                self.saved.push(self.gstate.clone());
                self.gstate.ctm = matrix.then(&self.gstate.ctm);
                self.frames.push(Frame {
                    data,
                    pos: 0,
                    resources,
                    floor: self.saved.len(),
                    overflow: 0,
                    form: Some(id.unwrap_or((0, 0))),
                    fonts: HashMap::new(),
                });
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn show(&mut self, bytes: &[u8]) -> Result<(), ReadError> {
        let Some(index) = self.gstate.font else {
            return Ok(());
        };
        let Some(font) = self.state.fonts.get(index as usize).map(Rc::clone) else {
            return Ok(());
        };
        let font_size = self.gstate.font_size;
        let h_scale = self.gstate.h_scale;
        let stroked = self.gstate.render_mode == 2;
        let mut rest = bytes;
        while !rest.is_empty() {
            if self.out.glyphs.len() >= MAX_PAGE_GLYPHS {
                self.out.truncated = true;
                return Ok(());
            }
            let (code, len) = font.next_code(rest);
            rest = &rest[len.min(rest.len())..];
            self.scratch.clear();
            let mapped = font.decode(code, len, &mut self.scratch);
            let width = font.width(code, self.scratch.chars().next());
            let matrix = self.tm.then(&self.gstate.ctm);
            self.record(&matrix, &font, index, mapped, width, stroked)?;
            let spacing = if font.is_word_space(code, len) {
                self.gstate.word_spacing
            } else {
                0.0
            };
            let tx = (width * font_size + self.gstate.char_spacing + spacing) * h_scale;
            self.tm.translate(tx, 0.0);
        }
        Ok(())
    }

    fn record(
        &mut self,
        matrix: &Matrix,
        font: &Font,
        index: u32,
        mapped: bool,
        width: f64,
        stroked: bool,
    ) -> Result<(), ReadError> {
        let font_size = self.gstate.font_size;
        let (x, y) = matrix.apply(0.0, self.gstate.rise);
        let (ax, ay) = matrix.apply_vector(width * font_size * self.gstate.h_scale, 0.0);
        let (sx, sy) = matrix.apply_vector(0.0, font_size * font.size_scale);
        let size = sx.hypot(sy);
        let advance = ax.hypot(ay);
        let (ux, uy) = if advance > 1e-9 {
            (ax / advance, ay / advance)
        } else {
            let (dx, dy) = matrix.apply_vector(1.0, 0.0);
            let length = dx.hypot(dy);
            if length > 1e-12 {
                (dx / length, dy / length)
            } else {
                (1.0, 0.0)
            }
        };
        if !mapped {
            self.out.unmapped += 1;
        }
        if ![x, y, size, advance, ux, uy]
            .iter()
            .all(|value| value.is_finite())
            || size <= 1e-3
        {
            return Ok(());
        }
        let start = self.out.text.len();
        normalize_into(&self.scratch, &mut self.out.text);
        let mut flags = 0;
        if self.out.text[start..].chars().all(char::is_whitespace) {
            self.out.text.truncate(start);
            if mapped {
                flags |= SPACE;
            }
        }
        if self.chunk_start {
            flags |= CHUNK_START;
            self.chunk_start = false;
        }
        if stroked {
            flags |= STROKED;
        }
        let end = self.out.text.len();
        self.state.budget.charge_glyph(end - start)?;
        self.out.glyphs.push(Glyph {
            x,
            y,
            ux,
            uy,
            width: advance,
            size,
            text_start: u32::try_from(start).unwrap_or(u32::MAX),
            text_end: u32::try_from(end).unwrap_or(u32::MAX),
            font: index,
            flags,
        });
        Ok(())
    }
}

fn matrix_from_objects(doc: &Document, values: &[Object]) -> Option<Matrix> {
    if values.len() != 6 {
        return None;
    }
    let mut matrix = [0.0; 6];
    for (slot, value) in matrix.iter_mut().zip(values) {
        *slot = resolve(doc, value).and_then(number)?;
    }
    Some(Matrix(matrix))
}

/// Builds an operand from a token, reading a whole array or skipping a whole dictionary.
fn read_operand(token: Token<'_>, data: &[u8], pos: &mut usize) -> Operand {
    match token {
        Token::Number(value) => Operand::Number(value),
        Token::Name(name) => Operand::Name(name),
        Token::String(bytes) => Operand::String(bytes),
        Token::ArrayStart => read_array(data, pos),
        Token::DictStart => {
            skip_dict(data, pos);
            Operand::Other
        }
        Token::ArrayEnd | Token::DictEnd | Token::Keyword(_) => Operand::Other,
    }
}

/// Reads an array whose `[` was consumed. An operator inside the array closes it, so a missing
/// `]` cannot swallow the rest of the stream.
fn read_array(data: &[u8], pos: &mut usize) -> Operand {
    let mut stack: Vec<Vec<Operand>> = vec![Vec::new()];
    loop {
        let before = *pos;
        let Some(token) = next_token(data, pos) else {
            break;
        };
        let item = match token {
            Token::ArrayStart => {
                if stack.len() < MAX_ARRAY_DEPTH {
                    stack.push(Vec::new());
                    continue;
                }
                skip_array(data, pos);
                Operand::Other
            }
            Token::ArrayEnd => {
                let done = stack.pop().unwrap_or_default();
                match stack.last_mut() {
                    Some(parent) => {
                        if parent.len() < MAX_ARRAY_ITEMS {
                            parent.push(Operand::Array(done));
                        }
                        continue;
                    }
                    None => return Operand::Array(done),
                }
            }
            Token::DictStart => {
                skip_dict(data, pos);
                Operand::Other
            }
            Token::DictEnd => continue,
            Token::Keyword(b"true" | b"false" | b"null") => Operand::Other,
            Token::Keyword(_) => {
                *pos = before;
                break;
            }
            Token::Number(value) => Operand::Number(value),
            Token::Name(name) => Operand::Name(name),
            Token::String(bytes) => Operand::String(bytes),
        };
        if let Some(top) = stack.last_mut()
            && top.len() < MAX_ARRAY_ITEMS
        {
            top.push(item);
        }
    }
    // Close arrays left open at the end of the data or before an operator.
    let mut done = stack.pop().unwrap_or_default();
    while let Some(mut parent) = stack.pop() {
        parent.push(Operand::Array(done));
        done = parent;
    }
    Operand::Array(done)
}

#[cfg(test)]
mod tests {
    use lopdf::{Dictionary, Document, Object, Stream, dictionary};

    use super::{DocState, Matrix, PageContent, interpret_page};
    use crate::reader::pdf::objects::Budget;

    fn run(content: &[u8], extra: impl FnOnce(&mut Document, &mut Dictionary)) -> PageContent {
        let mut doc = Document::new();
        let font = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "FirstChar" => 32,
            "Widths" => vec![Object::Integer(250); 96],
        });
        let mut resources = dictionary! { "Font" => dictionary! { "F1" => font } };
        extra(&mut doc, &mut resources);
        let resources_id = doc.add_object(resources);
        let content = doc.add_object(Stream::new(Dictionary::new(), content.to_vec()));
        let doc = doc;
        let resources = doc.get_dictionary(resources_id).ok();
        let mut state = DocState::new(&doc, Budget::new(1 << 20, 1 << 20));
        interpret_page(&mut state, &[content], resources, Matrix::IDENTITY).unwrap()
    }

    fn texts(page: &PageContent) -> String {
        page.glyphs
            .iter()
            .map(|glyph| page.glyph_text(glyph))
            .collect()
    }

    #[test]
    fn repeated_font_dictionaries_charge_retained_width_allocations() {
        let mut doc = Document::new();
        let widths = (0..1024)
            .flat_map(|code| {
                [
                    Object::Integer(code),
                    Object::Integer(code),
                    Object::Integer(500),
                ]
            })
            .collect::<Vec<_>>();
        let widths = doc.add_object(Object::Array(widths));
        let descendant = doc.add_object(dictionary! { "Subtype" => "CIDFontType2", "W" => widths });
        let ids = (0..16).map(|_| doc.add_object(dictionary! {
            "Subtype" => "Type0", "Encoding" => "Identity-H", "DescendantFonts" => vec![Object::Reference(descendant)]
        })).collect::<Vec<_>>();
        let limit = 128 * 1024;
        let mut state = DocState::new(&doc, Budget::new(1 << 20, limit));
        let first = doc.get_object(ids[0]).unwrap();
        assert_eq!(state.font_index(first).unwrap(), 0);
        assert_eq!(
            state.font_index(first).unwrap(),
            0,
            "cached fonts are not charged again"
        );
        let error = ids[1..]
            .iter()
            .find_map(|id| state.font_index(doc.get_object(*id).unwrap()).err());
        assert_eq!(
            error,
            Some(super::ReadError::DecompressedTooLarge { limit })
        );
    }

    #[test]
    fn positions_glyphs_with_text_state() {
        let page = run(
            b"BT /F1 10 Tf 2 Tc 100 200 Td (Ab) Tj [(c) -1000 (d)] TJ 0 -12 Td 200 Tz (e) Tj ET",
            |_, _| {},
        );
        assert_eq!(texts(&page), "Abcde");
        let x: Vec<f64> = page.glyphs.iter().map(|glyph| glyph.x).collect();
        // Each glyph advances 0.25 * 10 + 2 = 4.5; the TJ adjustment adds 10.
        assert_eq!(x, vec![100.0, 104.5, 109.0, 123.5, 100.0]);
        assert_eq!(page.glyphs[4].y, 188.0);
        assert!((page.glyphs[4].width - 5.0).abs() < 1e-9);
        assert!(
            page.glyphs
                .iter()
                .all(|glyph| (glyph.size - 10.0).abs() < 1e-9)
        );
    }

    #[test]
    fn graphics_state_and_forms_transform_text() {
        let page = run(
            b"q 2 0 0 2 10 10 cm BT /F1 5 Tf (a) Tj ET Q /Fm Do BT /F1 5 Tf 1 0 0 1 0 0 Tm (c) Tj ET",
            |doc, resources| {
                let form = doc.add_object(Stream::new(
                    dictionary! { "Type" => "XObject", "Subtype" => "Form", "Matrix" => [1, 0, 0, 1, 50, 0].map(Object::Integer).to_vec() },
                    b"q 3 0 0 3 0 0 cm BT /F1 5 Tf (b) Tj ET".to_vec(),
                ));
                let image = doc.add_object(Stream::new(
                    dictionary! { "Type" => "XObject", "Subtype" => "Image" },
                    Vec::new(),
                ));
                resources.set("XObject", dictionary! { "Fm" => form, "Im" => image });
            },
        );
        assert_eq!(texts(&page), "abc");
        assert_eq!((page.glyphs[0].x, page.glyphs[0].size), (10.0, 10.0));
        assert_eq!((page.glyphs[1].x, page.glyphs[1].size), (50.0, 15.0));
        // The form's unbalanced `q` does not leak into the page.
        assert_eq!((page.glyphs[2].x, page.glyphs[2].size), (0.0, 5.0));
    }

    #[test]
    fn counts_images_and_skips_inline_image_data() {
        let page = run(
            b"/Im Do BI /W 1 /H 1 /BPC 8 /CS /G ID \x29 EI BT /F1 9 Tf (x) Tj ET",
            |doc, resources| {
                let image = doc.add_object(Stream::new(
                    dictionary! { "Type" => "XObject", "Subtype" => "Image" },
                    Vec::new(),
                ));
                resources.set("XObject", dictionary! { "Im" => image });
            },
        );
        assert_eq!(page.images, 2);
        assert_eq!(texts(&page), "x");
    }

    #[test]
    fn self_referencing_forms_stop() {
        let page = run(b"/Fm Do", |doc, resources| {
            let id = doc.new_object_id();
            doc.objects.insert(
                id,
                Object::Stream(Stream::new(
                    dictionary! { "Subtype" => "Form", "Resources" => dictionary! { "XObject" => dictionary! { "Fm" => id } } },
                    b"/Fm Do".to_vec(),
                )),
            );
            resources.set("XObject", dictionary! { "Fm" => id });
        });
        assert!(page.glyphs.is_empty());
    }

    #[test]
    fn malformed_operands_do_not_stop_the_page() {
        let page = run(
            b"BT /F1 10 Tf [(a) Tj (b) Tj ] ] >> << /K [1 2 >> TJ (c) Tj ET",
            |_, _| {},
        );
        assert_eq!(texts(&page), "bc");
    }

    #[test]
    fn matrix_composition_applies_left_first() {
        let scale = Matrix([2.0, 0.0, 0.0, 2.0, 0.0, 0.0]);
        let shift = Matrix([1.0, 0.0, 0.0, 1.0, 5.0, 0.0]);
        assert_eq!(scale.then(&shift).apply(1.0, 1.0), (7.0, 2.0));
        assert_eq!(shift.then(&scale).apply(1.0, 1.0), (12.0, 2.0));
    }
}

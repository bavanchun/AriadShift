//! Conservative classification of existing PDF text, including Form XObjects.
use crate::{PdfDoc, PdfError, ToolCtx, document::DecodeBudget, xobject};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, content::Content};
use std::collections::HashSet;

/// Existing visible and invisible text and whether text also changes clipping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextPresence {
    /// Text painted in a visible render mode.
    pub visible: bool,
    /// Text in invisible render mode 3.
    pub invisible: bool,
    /// Text that changes the clipping path (render modes 4 through 7).
    pub clipping: bool,
    /// Text found in a Form XObject, which cannot be removed by a page-only rewrite.
    pub in_form: bool,
}
impl TextPresence {
    /// Whether the page contains any text or text clipping operations.
    #[must_use]
    pub const fn any(self) -> bool {
        self.visible || self.invisible || self.clipping
    }
}

/// Classify text shown by a page and its nested forms, subject to PDF decode limits.
pub fn classify_text(
    pdf: &PdfDoc,
    page_number: u32,
    ctx: &ToolCtx,
) -> Result<TextPresence, PdfError> {
    ctx.check()?;
    let index = page_number
        .checked_sub(1)
        .ok_or_else(|| PdfError::InvalidParams("invalid page number".into()))?
        as usize;
    let ids = crate::tools::rotate::collect_page_ids(pdf.inner(), ctx)?;
    let id = *ids
        .get(index)
        .ok_or_else(|| PdfError::InvalidParams("page number outside document".into()))?;
    let doc = pdf.inner();
    let page = doc
        .get_object(id)
        .and_then(Object::as_dict)
        .map_err(|_| PdfError::Malformed)?;
    let resources = xobject::resolve_page_resources(doc, id);
    let mut state = Walker {
        doc,
        ctx,
        budget: pdf.decode_budget(),
        active: HashSet::new(),
        operations: 0,
        result: TextPresence::default(),
    };
    if let Ok(contents) = page.get(b"Contents") {
        let object = state.resolve(contents)?;
        let pieces: Vec<Object> = match object {
            Object::Array(items) => items.clone(),
            Object::Stream(_) => vec![contents.clone()],
            _ => return Err(PdfError::Malformed),
        };
        let mut bytes = Vec::new();
        for piece in &pieces {
            let stream = state
                .resolve(piece)?
                .as_stream()
                .map_err(|_| PdfError::Malformed)?
                .clone();
            bytes.extend_from_slice(&state.decode(&stream)?);
            bytes.push(b'\n');
        }
        state.walk(&bytes, &resources, 0, 0, false)?;
    }
    Ok(state.result)
}

struct Walker<'a> {
    doc: &'a Document,
    ctx: &'a ToolCtx,
    budget: DecodeBudget,
    active: HashSet<ObjectId>,
    operations: usize,
    result: TextPresence,
}
impl<'a> Walker<'a> {
    fn resolve<'b>(&self, object: &'b Object) -> Result<&'b Object, PdfError>
    where
        'a: 'b,
    {
        self.doc
            .dereference(object)
            .map(|(_, value)| value)
            .map_err(|_| PdfError::Malformed)
    }
    fn decode(&mut self, stream: &Stream) -> Result<Vec<u8>, PdfError> {
        self.budget.decode(stream)?.ok_or(PdfError::Malformed)
    }
    fn walk(
        &mut self,
        bytes: &[u8],
        resources: &Dictionary,
        mut mode: i64,
        depth: usize,
        in_form: bool,
    ) -> Result<(), PdfError> {
        if depth > 64 {
            return Err(PdfError::Malformed);
        }
        let content = decode_checked(bytes, self.ctx)?;
        let mut stack = Vec::new();
        let mut marked_depth = 0usize;
        let mut compatibility_depth = 0usize;
        let mut in_text = false;
        for op in content.operations {
            self.ctx.check()?;
            self.operations += 1;
            if self.operations > 1_000_000 {
                return Err(PdfError::LimitExceeded(
                    ariad_core::reader::pdf::ReadError::TooManyObjects { limit: 1_000_000 },
                ));
            }
            match op.operator.as_str() {
                "q" => stack.push(mode),
                "Q" => mode = stack.pop().ok_or(PdfError::Malformed)?,
                "BMC" => marked_depth = marked_depth.checked_add(1).ok_or(PdfError::Malformed)?,
                "EMC" => marked_depth = marked_depth.checked_sub(1).ok_or(PdfError::Malformed)?,
                "BX" => {
                    compatibility_depth = compatibility_depth
                        .checked_add(1)
                        .ok_or(PdfError::Malformed)?
                }
                "EX" => {
                    compatibility_depth = compatibility_depth
                        .checked_sub(1)
                        .ok_or(PdfError::Malformed)?
                }
                "BT" if !in_text => in_text = true,
                "ET" if in_text => in_text = false,
                "BT" | "ET" => return Err(PdfError::Malformed),
                "Tr" => {
                    if op.operands.len() != 1 {
                        return Err(PdfError::Malformed);
                    }
                    mode = op.operands[0].as_i64().map_err(|_| PdfError::Malformed)?;
                    if !(0..=7).contains(&mode) {
                        return Err(PdfError::Malformed);
                    }
                }
                "Tj" | "TJ" | "'" | "\"" => {
                    let expected = if op.operator == "\"" { 3 } else { 1 };
                    if !in_text || op.operands.len() != expected {
                        return Err(PdfError::Malformed);
                    }
                    let last = op.operands.last().ok_or(PdfError::Malformed)?;
                    let present = match last {
                        Object::String(value, _) => !value.is_empty(),
                        Object::Array(items) if op.operator == "TJ" => {
                            if items.iter().any(|item| {
                                !matches!(
                                    item,
                                    Object::String(_, _) | Object::Integer(_) | Object::Real(_)
                                )
                            }) {
                                return Err(PdfError::Malformed);
                            }
                            items.iter().any(|item| matches!(item, Object::String(value, _) if !value.is_empty()))
                        }
                        _ => return Err(PdfError::Malformed),
                    };
                    if present {
                        self.result.in_form |= in_form;
                        self.result.visible |= matches!(mode, 0..=2 | 4..=6);
                        self.result.invisible |= matches!(mode, 3 | 7);
                        self.result.clipping |= mode >= 4;
                    }
                }
                "BDC" => {
                    marked_depth = marked_depth.checked_add(1).ok_or(PdfError::Malformed)?;
                    let property = op.operands.last().ok_or(PdfError::Malformed)?;
                    let dict = match property {
                        Object::Dictionary(dict) => dict,
                        Object::Name(name) => {
                            let properties = self
                                .resolve(
                                    resources
                                        .get(b"Properties")
                                        .map_err(|_| PdfError::Malformed)?,
                                )?
                                .as_dict()
                                .map_err(|_| PdfError::Malformed)?;
                            self.resolve(properties.get(name).map_err(|_| PdfError::Malformed)?)?
                                .as_dict()
                                .map_err(|_| PdfError::Malformed)?
                        }
                        _ => return Err(PdfError::Malformed),
                    };
                    if let Ok(value) = dict.get(b"ActualText") {
                        let text = self
                            .resolve(value)?
                            .as_str()
                            .map_err(|_| PdfError::Malformed)?;
                        if !text.is_empty() {
                            self.result.invisible = true;
                            self.result.in_form |= in_form;
                        }
                    }
                }
                "Do" => {
                    if in_text || op.operands.len() != 1 {
                        return Err(PdfError::Malformed);
                    }
                    let name = op
                        .operands
                        .first()
                        .ok_or(PdfError::Malformed)?
                        .as_name()
                        .map_err(|_| PdfError::Malformed)?;
                    let xobjects = self
                        .resolve(resources.get(b"XObject").map_err(|_| PdfError::Malformed)?)?
                        .as_dict()
                        .map_err(|_| PdfError::Malformed)?;
                    let object = xobjects.get(name).map_err(|_| PdfError::Malformed)?;
                    let (id, value) = self
                        .doc
                        .dereference(object)
                        .map_err(|_| PdfError::Malformed)?;
                    let stream = value.as_stream().map_err(|_| PdfError::Malformed)?.clone();
                    match stream
                        .dict
                        .get(b"Subtype")
                        .and_then(Object::as_name)
                        .map_err(|_| PdfError::Malformed)?
                    {
                        b"Image" => {}
                        b"Form" => {
                            let id = id.ok_or(PdfError::Malformed)?;
                            if !self.active.insert(id) {
                                return Err(PdfError::Malformed);
                            }
                            let form_resources = match stream.dict.get(b"Resources") {
                                Ok(value) => self
                                    .resolve(value)?
                                    .as_dict()
                                    .map_err(|_| PdfError::Malformed)?
                                    .clone(),
                                Err(_) => resources.clone(),
                            };
                            let data = self.decode(&stream)?;
                            self.walk(&data, &form_resources, mode, depth + 1, true)?;
                            self.active.remove(&id);
                        }
                        _ => return Err(PdfError::Malformed),
                    }
                }
                _ => {}
            }
        }
        if !stack.is_empty() || marked_depth != 0 || compatibility_depth != 0 || in_text {
            return Err(PdfError::Malformed);
        }
        Ok(())
    }
}

/// Remove safely removable invisible text from a page, leaving visible text and graphics intact.
/// Pages with text clipping, form-contained text or mixed visible/invisible text in one text
/// object are refused when safe rewriting cannot be established.
pub fn remove_invisible_text(
    pdf: &mut PdfDoc,
    page_number: u32,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    let mut output = pdf.clone();
    remove_invisible_text_in_place(&mut output, page_number, ctx)?;
    *pdf = output;
    Ok(())
}

/// Remove a layer without cloning the document; discard it if an error occurs.
pub fn remove_invisible_text_in_place(
    pdf: &mut PdfDoc,
    page_number: u32,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    let presence = classify_text(pdf, page_number, ctx)?;
    if !presence.invisible {
        return Ok(());
    }
    if presence.clipping || presence.in_form {
        return Err(PdfError::InvalidParams(
            "cannot safely remove this text layer".into(),
        ));
    }
    remove_page_text(pdf, page_number, ctx)
}

fn remove_page_text(pdf: &mut PdfDoc, page_number: u32, ctx: &ToolCtx) -> Result<(), PdfError> {
    use lopdf::content::Operation;
    let index = page_number.checked_sub(1).ok_or(PdfError::Malformed)? as usize;
    let page_ids = crate::tools::rotate::collect_page_ids(pdf.inner(), ctx)?;
    let id = *page_ids.get(index).ok_or(PdfError::Malformed)?;
    let doc = pdf.inner();
    let page = doc
        .get_object(id)
        .and_then(Object::as_dict)
        .map_err(|_| PdfError::Malformed)?;
    let contents = page.get(b"Contents").map_err(|_| PdfError::Malformed)?;
    let items: Vec<Object> = match doc
        .dereference(contents)
        .map_err(|_| PdfError::Malformed)?
        .1
    {
        Object::Array(items) => items.clone(),
        Object::Stream(_) => vec![contents.clone()],
        _ => return Err(PdfError::Malformed),
    };
    let mut budget = pdf.decode_budget();
    let mut bytes = Vec::new();
    for item in &items {
        ctx.check()?;
        let stream = doc
            .dereference(item)
            .map_err(|_| PdfError::Malformed)?
            .1
            .as_stream()
            .map_err(|_| PdfError::Malformed)?;
        bytes.extend_from_slice(&budget.decode(stream)?.ok_or(PdfError::Malformed)?);
        bytes.push(b'\n');
    }
    let content = decode_checked(&bytes, ctx)?;
    let ops = content.operations;
    let mut unwrap = HashSet::new();
    for (i, op) in ops.iter().enumerate() {
        if op.operator != "BDC" {
            continue;
        }
        let dict = op
            .operands
            .last()
            .and_then(|o| o.as_dict().ok())
            .ok_or_else(|| PdfError::InvalidParams("cannot safely remove marked text".into()))?;
        if !dict.has(b"ActualText") {
            return Err(PdfError::InvalidParams(
                "cannot safely remove marked text".into(),
            ));
        }
        // The layer writer emits a self-contained span immediately around one text object.
        // Other ActualText arrangements may represent visible graphics and are not rewritten.
        if dict.len() != 1
            || op.operands.len() != 2
            || op.operands[0].as_name().ok() != Some(b"Span")
            || ops.get(i + 1).is_none_or(|next| next.operator != "BT")
        {
            return Err(PdfError::InvalidParams(
                "cannot safely remove marked text".into(),
            ));
        }
        let Some(end) = ops[i + 2..]
            .iter()
            .position(|candidate| candidate.operator == "ET")
            .map(|n| n + i + 2)
        else {
            return Err(PdfError::Malformed);
        };
        if ops.get(end + 1).is_none_or(|next| next.operator != "EMC")
            || ops[i + 2..end].iter().any(|candidate| {
                !matches!(
                    candidate.operator.as_str(),
                    "Tf" | "Tz" | "Tr" | "Tm" | "Tj"
                )
            })
            || !is_exclusively_invisible_span(&ops[i + 2..end])
        {
            return Err(PdfError::InvalidParams(
                "cannot safely remove marked text".into(),
            ));
        }
        unwrap.insert(i);
        unwrap.insert(end + 1);
    }
    let mut mode = 0_i64;
    let mut stack = Vec::new();
    let mut text_start = None;
    let mut visible = false;
    let mut invisible = false;
    let mut unsafe_object = false;
    for (index, op) in ops.iter().enumerate() {
        ctx.check()?;
        if text_start.is_some()
            && !matches!(
                op.operator.as_str(),
                "ET" | "Tf"
                    | "Tc"
                    | "Tw"
                    | "Tz"
                    | "TL"
                    | "Tr"
                    | "Ts"
                    | "Td"
                    | "TD"
                    | "Tm"
                    | "T*"
                    | "Tj"
                    | "TJ"
                    | "'"
                    | "\""
            )
        {
            unsafe_object = true;
        }
        match op.operator.as_str() {
            "q" => stack.push(mode),
            "Q" => mode = stack.pop().ok_or(PdfError::Malformed)?,
            "Tr" => {
                mode = op
                    .operands
                    .first()
                    .and_then(|v| v.as_i64().ok())
                    .ok_or(PdfError::Malformed)?
            }
            "BT" => {
                text_start = Some(index);
                visible = false;
                invisible = false;
                unsafe_object = false;
            }
            "ET" => {
                if text_start.take().is_none() {
                    return Err(PdfError::Malformed);
                }
                if invisible && (visible || unsafe_object) {
                    return Err(PdfError::InvalidParams(
                        "cannot safely remove mixed text".into(),
                    ));
                }
            }
            "Tj" | "TJ" | "'" | "\"" => {
                if mode == 3 {
                    invisible = true;
                } else {
                    visible = true;
                }
            }
            _ => {}
        }
    }
    let mut output = Vec::with_capacity(ops.len());
    mode = 0;
    stack.clear();
    for (index, op) in ops.into_iter().enumerate() {
        ctx.check()?;
        match op.operator.as_str() {
            "q" => stack.push(mode),
            "Q" => mode = stack.pop().ok_or(PdfError::Malformed)?,
            "Tr" => {
                mode = op
                    .operands
                    .first()
                    .and_then(|v| v.as_i64().ok())
                    .ok_or(PdfError::Malformed)?
            }
            _ => {}
        }
        if unwrap.contains(&index) {
            continue;
        }
        if mode == 3 {
            match op.operator.as_str() {
                "Tj" | "TJ" => continue,
                "'" => {
                    output.push(Operation::new("T*", vec![]));
                    continue;
                }
                "\"" => {
                    if op.operands.len() != 3 {
                        return Err(PdfError::Malformed);
                    }
                    output.push(Operation::new("Tw", vec![op.operands[0].clone()]));
                    output.push(Operation::new("Tc", vec![op.operands[1].clone()]));
                    output.push(Operation::new("T*", vec![]));
                    continue;
                }
                _ => {}
            }
        }
        output.push(op);
    }
    let rewritten = Content { operations: output }
        .encode()
        .map_err(|_| PdfError::Malformed)?;
    budget.charge(rewritten.len())?;
    let doc = pdf.inner_mut()?;
    let stream = doc.add_object(Stream::new(lopdf::dictionary! {}, rewritten));
    doc.get_object_mut(id)
        .and_then(Object::as_dict_mut)
        .map_err(|_| PdfError::Malformed)?
        .set("Contents", stream);
    Ok(())
}

fn is_exclusively_invisible_span(ops: &[lopdf::content::Operation]) -> bool {
    let mut mode = None;
    let mut has_text = false;
    for op in ops {
        match op.operator.as_str() {
            "Tr" => mode = op.operands.first().and_then(|value| value.as_i64().ok()),
            "Tj" => {
                if mode != Some(3) {
                    return false;
                }
                has_text = true;
            }
            _ => {}
        }
    }
    has_text
}

// Count operations before strict collection so a small stream cannot allocate an
// unbounded number of operation objects. The strict pass still rejects any trailing
// malformed bytes that the streaming parser intentionally ignores.
pub(crate) fn decode_checked(bytes: &[u8], ctx: &ToolCtx) -> Result<Content, PdfError> {
    const MAX_OPERATIONS: usize = 1_000_000;
    for (count, operation) in Content::decode_iter(bytes).enumerate() {
        ctx.check()?;
        operation.map_err(|_| PdfError::Malformed)?;
        if count >= MAX_OPERATIONS {
            return Err(PdfError::LimitExceeded(
                ariad_core::reader::pdf::ReadError::TooManyObjects {
                    limit: MAX_OPERATIONS,
                },
            ));
        }
    }
    Content::decode_strict(bytes).map_err(|_| PdfError::Malformed)
}

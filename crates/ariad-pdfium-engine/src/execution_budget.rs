//! Bound execution of content resources before PDFium opens a document.
//!
//! The pre-flight charges content operators, form, pattern and soft-mask
//! invocations, Type3 glyph procedures and inline image data. It does not
//! decode the auxiliary streams PDFium reads while it parses a page:
//! embedded font programs (`/FontFile`, `/FontFile2`, `/FontFile3`),
//! `/ICCBased` profiles, `/Indexed` lookup streams, and sampled or
//! PostScript function streams (shadings, `/Separation` and `/DeviceN` tint
//! transforms). Their CPU and memory use is bounded only by PDFium's own
//! decoder limits and the engine process ceiling (memory and time limits
//! applied by the host launcher).
use std::cell::OnceCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use ariad_core::protocol::ErrorCode;
use ariad_engine_io::EngineFailure;
use lopdf::{Dictionary, Object, ObjectId};
use sha2::{Digest, Sha256};

use crate::{
    extract_images::{preflight_page_tree, resolve},
    output::ToolContext,
};

/// Maximum execution work units allowed per page before refusing to load.
pub(crate) const MAX_PAGE_WORK: usize = 100_000;
/// Maximum execution work units allowed across all selected pages in a document.
pub(crate) const MAX_DOCUMENT_WORK: usize = 1_000_000;
/// Maximum nested content-stream call depth (`Do`, `PatternType 1`, `SMask /G`, `CharProcs`).
pub(crate) const MAX_CONTENT_NESTING_DEPTH: usize = 8;

#[derive(Clone, Debug)]
pub(crate) struct Call {
    pub op: Vec<u8>,
    pub name: Vec<u8>,
}

/// Per-scope operation and inline-image costs for a single content stream.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ScopedStreamFacts {
    pub ops: usize,
    pub inline_pixels: u64,
    pub inline_bytes: u64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Facts {
    pub ops: usize,
    pub glyphs: usize,
    pub inline_pixels: u64,
    pub inline_bytes: u64,
    pub calls: Vec<Call>,
}

impl Facts {
    pub(crate) fn scoped(&self) -> ScopedStreamFacts {
        ScopedStreamFacts {
            ops: self.ops,
            inline_pixels: self.inline_pixels,
            inline_bytes: self.inline_bytes,
        }
    }
}

fn hash_len(hasher: &mut Sha256, len: usize) {
    hasher.update(u64::try_from(len).unwrap_or(u64::MAX).to_be_bytes());
}

fn hash_bytes(hasher: &mut Sha256, bytes: &[u8]) {
    hash_len(hasher, bytes.len());
    hasher.update(bytes);
}

pub(crate) fn object_digest(object: &Object) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hash_object(&mut hasher, object);
    hasher.finalize().into()
}

fn hash_object(hasher: &mut Sha256, root: &Object) {
    enum Step<'a> {
        Obj(&'a Object),
        Bytes(&'a [u8]),
    }

    fn push_dict_entries<'a>(stack: &mut Vec<Step<'a>>, hasher: &mut Sha256, dict: &'a Dictionary) {
        let mut entries: Vec<(&[u8], &Object)> = dict
            .iter()
            .map(|(key, value)| (key.as_slice(), value))
            .collect();
        entries.sort_unstable_by_key(|(a, _)| *a);
        hash_len(hasher, entries.len());
        for (key, value) in entries.into_iter().rev() {
            stack.push(Step::Obj(value));
            stack.push(Step::Bytes(key));
        }
    }

    let mut stack = vec![Step::Obj(root)];
    while let Some(step) = stack.pop() {
        match step {
            Step::Bytes(bytes) => hash_bytes(hasher, bytes),
            Step::Obj(object) => match object {
                Object::Null => hasher.update([0]),
                Object::Boolean(value) => hasher.update([1, u8::from(*value)]),
                Object::Integer(value) => {
                    hasher.update([2]);
                    hasher.update(value.to_be_bytes());
                }
                Object::Real(value) => {
                    hasher.update([3]);
                    hasher.update(value.to_bits().to_be_bytes());
                }
                Object::Name(name) => {
                    hasher.update([4]);
                    hash_bytes(hasher, name);
                }
                Object::String(bytes, _) => {
                    hasher.update([5]);
                    hash_bytes(hasher, bytes);
                }
                Object::Array(items) => {
                    hasher.update([6]);
                    hash_len(hasher, items.len());
                    for item in items.iter().rev() {
                        stack.push(Step::Obj(item));
                    }
                }
                Object::Dictionary(dict) => {
                    hasher.update([7]);
                    push_dict_entries(&mut stack, hasher, dict);
                }
                Object::Stream(stream) => {
                    hasher.update([8]);
                    stack.push(Step::Bytes(&stream.content));
                    push_dict_entries(&mut stack, hasher, &stream.dict);
                }
                Object::Reference((number, generation)) => {
                    hasher.update([9]);
                    hasher.update(number.to_be_bytes());
                    hasher.update(generation.to_be_bytes());
                }
            },
        }
    }
}

#[derive(Default)]
pub(crate) struct Category {
    entries: BTreeMap<Vec<u8>, Object>,
    digest: OnceCell<[u8; 32]>,
}

impl Clone for Category {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries.clone(),
            // Clear cached digest on value clone so future mutations cannot reuse it.
            digest: OnceCell::new(),
        }
    }
}

impl From<BTreeMap<Vec<u8>, Object>> for Category {
    fn from(entries: BTreeMap<Vec<u8>, Object>) -> Self {
        Self {
            entries,
            digest: OnceCell::new(),
        }
    }
}

impl FromIterator<(Vec<u8>, Object)> for Category {
    fn from_iter<T: IntoIterator<Item = (Vec<u8>, Object)>>(iter: T) -> Self {
        Self::from(BTreeMap::from_iter(iter))
    }
}

impl Deref for Category {
    type Target = BTreeMap<Vec<u8>, Object>;

    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

impl DerefMut for Category {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.digest.take();
        &mut self.entries
    }
}

impl Category {
    fn digest(&self) -> [u8; 32] {
        *self.digest.get_or_init(|| {
            let mut hasher = Sha256::new();
            hasher.update(b"ariad.execution_budget.category.v1");
            hash_len(&mut hasher, self.entries.len());
            for (name, value) in &self.entries {
                hash_bytes(&mut hasher, name);
                hash_object(&mut hasher, value);
            }
            hasher.finalize().into()
        })
    }
}

#[derive(Default)]
pub(crate) struct Categories {
    entries: BTreeMap<Vec<u8>, Rc<Category>>,
    digest: OnceCell<[u8; 32]>,
}

impl Clone for Categories {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries.clone(),
            // Clear cached digest on value clone so child overrides recompute it.
            digest: OnceCell::new(),
        }
    }
}

impl IntoIterator for Categories {
    type Item = (Vec<u8>, Rc<Category>);
    type IntoIter = std::collections::btree_map::IntoIter<Vec<u8>, Rc<Category>>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

impl Deref for Categories {
    type Target = BTreeMap<Vec<u8>, Rc<Category>>;

    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

impl DerefMut for Categories {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.digest.take();
        &mut self.entries
    }
}

impl Categories {
    fn digest(&self) -> [u8; 32] {
        *self.digest.get_or_init(|| {
            let mut hasher = Sha256::new();
            hasher.update(b"ariad.execution_budget.categories.v1");
            hash_len(&mut hasher, self.entries.len());
            for (name, category) in &self.entries {
                hash_bytes(&mut hasher, name);
                hasher.update(category.digest());
            }
            hasher.finalize().into()
        })
    }
}

#[derive(Clone, Default)]
pub(crate) struct Scope {
    page: Rc<Categories>,
    physical_page: Rc<Categories>,
    current: Rc<Categories>,
    inherited_fonts: Vec<(Object, Rc<Categories>)>,
    digest: OnceCell<[u8; 32]>,
}

impl Scope {
    pub(crate) fn from_page(
        doc: &lopdf::Document,
        resources: Option<&Object>,
    ) -> Result<Self, EngineFailure> {
        let current = Rc::new(categories(doc, resources)?);
        Ok(Self {
            page: current.clone(),
            physical_page: current.clone(),
            current,
            inherited_fonts: Vec::new(),
            digest: OnceCell::new(),
        })
    }
    pub(crate) fn child(
        &self,
        doc: &lopdf::Document,
        resources: Option<&Object>,
    ) -> Result<Self, EngineFailure> {
        let Some(resources) = resources else {
            return Ok(self.clone());
        };
        let mut current = (*self.page).clone();
        current.extend(categories(doc, Some(resources))?);
        let mut inherited_fonts = self.inherited_fonts.clone();
        if let Some(fonts) = self.current.get(b"Font".as_slice()) {
            let selected_resources = Rc::clone(&self.current);
            inherited_fonts.extend(
                fonts
                    .values()
                    .map(|font| (font.clone(), Rc::clone(&selected_resources))),
            );
        }
        Ok(Self {
            page: self.page.clone(),
            physical_page: self.physical_page.clone(),
            current: Rc::new(current),
            inherited_fonts,
            digest: OnceCell::new(),
        })
    }
    pub(crate) fn digest(&self) -> [u8; 32] {
        *self.digest.get_or_init(|| {
            let mut hasher = Sha256::new();
            hasher.update(b"ariad.execution_budget.scope.v1");
            hasher.update(self.page.digest());
            hasher.update(self.physical_page.digest());
            hasher.update(self.current.digest());
            hash_len(&mut hasher, self.inherited_fonts.len());
            for (font, resources) in &self.inherited_fonts {
                hash_object(&mut hasher, font);
                hasher.update(resources.digest());
            }
            hasher.finalize().into()
        })
    }
    pub(crate) fn resource_digest(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(b"ariad.execution_budget.resource_scope.v1");
        hasher.update(self.page.digest());
        hasher.update(self.physical_page.digest());
        hasher.update(self.current.digest());
        hasher.finalize().into()
    }
    pub(crate) fn colorspace_digest(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(b"ariad.execution_budget.colorspace_scope.v1");
        if let Some(cs) = self.current.get(b"ColorSpace".as_slice()) {
            hasher.update([1]);
            hasher.update(cs.digest());
        } else {
            hasher.update([0]);
        }
        hasher.finalize().into()
    }
    /// Digest covering only effective and fallback `/XObject` categories.
    pub(crate) fn xobject_digest(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(b"ariad.execution_budget.xobject_scope.v1");
        for categories in [&self.page, &self.physical_page, &self.current] {
            if let Some(xobjects) = categories.get(b"XObject".as_slice()) {
                hasher.update([1]);
                hasher.update(xobjects.digest());
            } else {
                hasher.update([0]);
            }
        }
        hasher.finalize().into()
    }
    /// Resolves a resource `/XObject` value to its indirect object ID.
    pub(crate) fn resolve_xobject_id(
        doc: &lopdf::Document,
        value: &Object,
    ) -> Result<ObjectId, EngineFailure> {
        let (id, _) = resolve(Some(doc), value)?;
        id.ok_or_else(EngineFailure::limit_exceeded)
    }
    /// Resolves a named XObject from the effective resource scope to its indirect object ID.
    pub(crate) fn xobject_id(
        &self,
        doc: &lopdf::Document,
        name: &[u8],
    ) -> Result<Option<ObjectId>, EngineFailure> {
        self.get(b"XObject", name)
            .map(|value| Self::resolve_xobject_id(doc, value))
            .transpose()
    }
    /// Iterates `(name, value)` pairs declared in the effective `/XObject` category.
    pub(crate) fn xobject_entries(&self) -> impl Iterator<Item = (&[u8], &Object)> + '_ {
        self.current_category(b"XObject")
            .into_iter()
            .flat_map(|values| values.iter().map(|(name, value)| (name.as_slice(), value)))
    }
    pub(crate) fn get(&self, category: &[u8], name: &[u8]) -> Option<&Object> {
        self.current
            .get(category)
            .and_then(|values| values.get(name))
    }
    pub(crate) fn current(&self) -> &Rc<Categories> {
        &self.current
    }
    pub(crate) fn current_category(&self, category: &[u8]) -> Option<&Rc<Category>> {
        self.current.get(category)
    }
    pub(crate) fn inherited_fonts(&self) -> &[(Object, Rc<Categories>)] {
        &self.inherited_fonts
    }
    pub(crate) fn mask_scope(&self) -> Self {
        Self {
            page: self.physical_page.clone(),
            physical_page: self.physical_page.clone(),
            current: self.physical_page.clone(),
            inherited_fonts: Vec::new(),
            digest: OnceCell::new(),
        }
    }
    pub(crate) fn type3_glyph_scope(
        &self,
        doc: &lopdf::Document,
        selected_resources: &Rc<Categories>,
        font: &Dictionary,
    ) -> Result<Self, EngineFailure> {
        let selected_scope = Self {
            page: selected_resources.clone(),
            physical_page: self.physical_page.clone(),
            current: selected_resources.clone(),
            inherited_fonts: self.inherited_fonts.clone(),
            digest: OnceCell::new(),
        };
        let mut glyph_scope = selected_scope.child(doc, font.get(b"Resources").ok())?;
        // PDFium treats even an empty font Resources dictionary as present.
        // Its selected font scope becomes the CharProc's fallback scope.
        glyph_scope.page = glyph_scope.current.clone();
        glyph_scope.digest = OnceCell::new();
        Ok(glyph_scope)
    }
}

fn categories(
    doc: &lopdf::Document,
    resources: Option<&Object>,
) -> Result<Categories, EngineFailure> {
    let mut out = Categories::default();
    let Some(resources) = resources else {
        return Ok(out);
    };
    let dict = resolve(Some(doc), resources)?
        .1
        .as_dict()
        .map_err(|_| EngineFailure::limit_exceeded())?;
    for category in [
        b"XObject".as_slice(),
        b"Pattern",
        b"ExtGState",
        b"Shading",
        b"Font",
        b"ColorSpace",
    ] {
        if let Ok(value) = dict.get(category) {
            let values = resolve(Some(doc), value)?
                .1
                .as_dict()
                .map_err(|_| EngineFailure::limit_exceeded())?;
            out.insert(
                category.to_vec(),
                Rc::new(
                    values
                        .iter()
                        .map(|(name, value)| (name.clone(), value.clone()))
                        .collect(),
                ),
            );
        }
    }
    Ok(out)
}

pub(crate) fn inherited_resources(
    doc: &lopdf::Document,
    page: ObjectId,
) -> Result<Option<&Object>, EngineFailure> {
    let mut current = page;
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(current) || seen.len() > 63 {
            return Err(EngineFailure::limit_exceeded());
        }
        let page = doc
            .get_dictionary(current)
            .map_err(|_| EngineFailure::limit_exceeded())?;
        if let Ok(resources) = page.get(b"Resources") {
            return Ok(Some(resources));
        }
        match page.get(b"Parent") {
            Ok(parent) => {
                current = parent
                    .as_reference()
                    .map_err(|_| EngineFailure::limit_exceeded())?
            }
            Err(_) => return Ok(None),
        }
    }
}

pub(crate) fn stream_ids(
    doc: &lopdf::Document,
    obj: &Object,
    allow_dict: bool,
    depth: usize,
    out: &mut Vec<ObjectId>,
) -> Result<(), EngineFailure> {
    if depth > 63 {
        return Err(EngineFailure::limit_exceeded());
    }
    match obj {
        Object::Reference(id) => {
            let obj = doc
                .get_object(*id)
                .map_err(|_| EngineFailure::limit_exceeded())?;
            if matches!(obj, Object::Stream(_)) {
                out.push(*id);
            } else {
                stream_ids(doc, obj, allow_dict, depth + 1, out)?;
            }
        }
        Object::Array(items) => {
            for item in items {
                stream_ids(doc, item, allow_dict, depth + 1, out)?;
            }
        }
        Object::Dictionary(dict) if allow_dict => {
            for (_, item) in dict.iter() {
                stream_ids(doc, item, true, depth + 1, out)?;
            }
        }
        Object::Null => (),
        _ => return Err(EngineFailure::limit_exceeded()),
    }
    Ok(())
}

fn limit(cost: usize, max: usize) -> Result<usize, EngineFailure> {
    if cost > max {
        return Err(EngineFailure::owned(
            ErrorCode::LimitExceeded,
            format!("Content execution cost {cost} exceeds limit {max}."),
        ));
    }
    Ok(cost)
}

fn check_inline_totals(
    ctx: &ToolContext<'_>,
    pixels: u64,
    bytes: u64,
) -> Result<(), EngineFailure> {
    if pixels > ctx.max_pixels() || bytes > ctx.max_pixels().saturating_mul(4) {
        return Err(EngineFailure::new(
            ErrorCode::LimitExceeded,
            "Page inline image totals exceed the limit.",
        ));
    }
    Ok(())
}

type MemoKey = (ObjectId, [u8; 32], usize, Vec<ObjectId>, bool);
type GlyphKey = ([u8; 32], usize, Vec<ObjectId>);

struct Calculator<'a, 'b> {
    ctx: &'a ToolContext<'b>,
    doc: &'a lopdf::Document,
    facts: &'a BTreeMap<ObjectId, Facts>,
    scoped_ops: Option<&'a HashMap<(ObjectId, [u8; 32]), ScopedStreamFacts>>,
    memo: HashMap<MemoKey, ScopedStreamFacts>,
    glyph_memo: HashMap<GlyphKey, ScopedStreamFacts>,
    page_contents: HashSet<ObjectId>,
}

impl Calculator<'_, '_> {
    fn add_cost(
        &self,
        base: ScopedStreamFacts,
        extra: ScopedStreamFacts,
    ) -> Result<ScopedStreamFacts, EngineFailure> {
        let ops = limit(
            base.ops
                .checked_add(extra.ops)
                .ok_or_else(EngineFailure::limit_exceeded)?,
            MAX_PAGE_WORK,
        )?;
        let inline_pixels = base
            .inline_pixels
            .checked_add(extra.inline_pixels)
            .ok_or_else(EngineFailure::limit_exceeded)?;
        let inline_bytes = base
            .inline_bytes
            .checked_add(extra.inline_bytes)
            .ok_or_else(EngineFailure::limit_exceeded)?;
        check_inline_totals(self.ctx, inline_pixels, inline_bytes)?;
        Ok(ScopedStreamFacts {
            ops,
            inline_pixels,
            inline_bytes,
        })
    }
    fn stream(
        &mut self,
        id: ObjectId,
        scope: &Scope,
        depth: usize,
        active: &mut HashSet<ObjectId>,
    ) -> Result<ScopedStreamFacts, EngineFailure> {
        self.stream_inner(id, scope, depth, active, false)
    }
    fn stream_inner(
        &mut self,
        id: ObjectId,
        scope: &Scope,
        depth: usize,
        active: &mut HashSet<ObjectId>,
        page_contents: bool,
    ) -> Result<ScopedStreamFacts, EngineFailure> {
        let page_contents = page_contents || self.page_contents.contains(&id);
        self.ctx.check()?;
        if depth > MAX_CONTENT_NESTING_DEPTH || !active.insert(id) {
            return Err(EngineFailure::limit_exceeded());
        }
        let mut ancestors: Vec<_> = active.iter().copied().collect();
        ancestors.sort_unstable();
        let key = (id, scope.digest(), depth, ancestors, page_contents);
        if let Some(&cost) = self.memo.get(&key) {
            active.remove(&id);
            return Ok(cost);
        }
        let facts = self
            .facts
            .get(&id)
            .ok_or_else(EngineFailure::limit_exceeded)?;
        let stream = self
            .doc
            .get_object(id)
            .and_then(Object::as_stream)
            .map_err(|_| EngineFailure::limit_exceeded())?;
        let scope = scope.child(
            self.doc,
            if page_contents {
                None
            } else {
                stream.dict.get(b"Resources").ok()
            },
        )?;
        let mut cost = match self.scoped_ops {
            Some(map) => map
                .get(&(id, scope.colorspace_digest()))
                .copied()
                .ok_or_else(EngineFailure::limit_exceeded)?,
            None => facts.scoped(),
        };
        check_inline_totals(self.ctx, cost.inline_pixels, cost.inline_bytes)?;
        for call in &facts.calls {
            let extra = match call.op.as_slice() {
                b"Do" => self.target(scope.get(b"XObject", &call.name), &scope, depth, active)?,
                b"scn" | b"SCN" => {
                    self.pattern(scope.get(b"Pattern", &call.name), &scope, depth, active)?
                }
                b"gs" => self.mask(scope.get(b"ExtGState", &call.name), &scope, depth, active)?,
                // The shading operator itself is already included in `facts.ops`.
                b"sh" => ScopedStreamFacts::default(),
                _ => ScopedStreamFacts::default(),
            };
            cost = self.add_cost(cost, extra)?;
        }
        if facts.glyphs > 0 {
            let mut ancestors: Vec<_> = active.iter().copied().collect();
            ancestors.sort_unstable();
            let glyph_key = (scope.digest(), depth, ancestors);
            let max_glyph = if let Some(&cost) = self.glyph_memo.get(&glyph_key) {
                cost
            } else {
                let mut max_glyph = ScopedStreamFacts::default();
                let fonts = scope
                    .current
                    .get(b"Font".as_slice())
                    .into_iter()
                    .flat_map(|fonts| fonts.values().map(|font| (font, &scope.current)))
                    .chain(
                        scope
                            .inherited_fonts
                            .iter()
                            .map(|(font, resources)| (font, resources)),
                    );
                for (font, selected_resources) in fonts {
                    let font = resolve(Some(self.doc), font)?
                        .1
                        .as_dict()
                        .map_err(|_| EngineFailure::limit_exceeded())?;
                    let subtype = font
                        .get(b"Subtype")
                        .ok()
                        .and_then(|v| resolve(Some(self.doc), v).ok())
                        .and_then(|(_, v)| v.as_name().ok());
                    if subtype != Some(b"Type3") {
                        continue;
                    }
                    let procs = font
                        .get(b"CharProcs")
                        .map_err(|_| EngineFailure::limit_exceeded())?;
                    let procs = resolve(Some(self.doc), procs)?
                        .1
                        .as_dict()
                        .map_err(|_| EngineFailure::limit_exceeded())?;
                    let glyph_scope =
                        scope.type3_glyph_scope(self.doc, selected_resources, font)?;
                    for (_, proc) in procs.iter() {
                        let (proc_id, _) = resolve(Some(self.doc), proc)?;
                        let glyph_cost = self.stream(
                            proc_id.ok_or_else(EngineFailure::limit_exceeded)?,
                            &glyph_scope,
                            depth + 1,
                            active,
                        )?;
                        max_glyph.ops = max_glyph.ops.max(glyph_cost.ops);
                        max_glyph.inline_pixels =
                            max_glyph.inline_pixels.max(glyph_cost.inline_pixels);
                        max_glyph.inline_bytes =
                            max_glyph.inline_bytes.max(glyph_cost.inline_bytes);
                    }
                }
                self.glyph_memo.insert(glyph_key, max_glyph);
                max_glyph
            };
            let glyph_uses =
                u64::try_from(facts.glyphs).map_err(|_| EngineFailure::limit_exceeded())?;
            let glyph_cost = ScopedStreamFacts {
                ops: facts
                    .glyphs
                    .checked_mul(max_glyph.ops)
                    .ok_or_else(EngineFailure::limit_exceeded)?,
                inline_pixels: glyph_uses
                    .checked_mul(max_glyph.inline_pixels)
                    .ok_or_else(EngineFailure::limit_exceeded)?,
                inline_bytes: glyph_uses
                    .checked_mul(max_glyph.inline_bytes)
                    .ok_or_else(EngineFailure::limit_exceeded)?,
            };
            cost = self.add_cost(cost, glyph_cost)?;
        }
        active.remove(&id);
        self.memo.insert(key, cost);
        Ok(cost)
    }
    fn target(
        &mut self,
        value: Option<&Object>,
        scope: &Scope,
        depth: usize,
        active: &mut HashSet<ObjectId>,
    ) -> Result<ScopedStreamFacts, EngineFailure> {
        let Some(value) = value else {
            return Ok(ScopedStreamFacts::default());
        };
        let (id, obj) = resolve(Some(self.doc), value)?;
        let stream = obj
            .as_stream()
            .map_err(|_| EngineFailure::limit_exceeded())?;
        let subtype = stream
            .dict
            .get(b"Subtype")
            .ok()
            .and_then(|v| resolve(Some(self.doc), v).ok())
            .and_then(|(_, v)| v.as_name().ok());
        match subtype {
            Some(b"Image") => Ok(ScopedStreamFacts::default()),
            Some(b"Form") => self.stream(
                id.ok_or_else(EngineFailure::limit_exceeded)?,
                scope,
                depth + 1,
                active,
            ),
            _ => Err(EngineFailure::limit_exceeded()),
        }
    }
    fn pattern(
        &mut self,
        value: Option<&Object>,
        scope: &Scope,
        depth: usize,
        active: &mut HashSet<ObjectId>,
    ) -> Result<ScopedStreamFacts, EngineFailure> {
        let Some(value) = value else {
            return Ok(ScopedStreamFacts::default());
        };
        let (id, obj) = resolve(Some(self.doc), value)?;
        let dict: &Dictionary = match obj {
            Object::Stream(stream) => &stream.dict,
            Object::Dictionary(dict) => dict,
            _ => return Err(EngineFailure::limit_exceeded()),
        };
        let kind = dict
            .get(b"PatternType")
            .ok()
            .and_then(|v| resolve(Some(self.doc), v).ok())
            .and_then(|(_, v)| v.as_i64().ok());
        match kind {
            Some(1) => self.stream(
                id.ok_or_else(EngineFailure::limit_exceeded)?,
                scope,
                depth + 1,
                active,
            ),
            Some(2) => Ok(ScopedStreamFacts {
                ops: 1,
                inline_pixels: 0,
                inline_bytes: 0,
            }),
            _ => Err(EngineFailure::limit_exceeded()),
        }
    }
    fn mask(
        &mut self,
        value: Option<&Object>,
        scope: &Scope,
        depth: usize,
        active: &mut HashSet<ObjectId>,
    ) -> Result<ScopedStreamFacts, EngineFailure> {
        let Some(value) = value else {
            return Ok(ScopedStreamFacts::default());
        };
        let dict = resolve(Some(self.doc), value)?
            .1
            .as_dict()
            .map_err(|_| EngineFailure::limit_exceeded())?;
        let Ok(mask) = dict.get(b"SMask") else {
            return Ok(ScopedStreamFacts::default());
        };
        let mask = resolve(Some(self.doc), mask)?.1;
        if matches!(mask, Object::Name(name) if name == b"None") {
            return Ok(ScopedStreamFacts::default());
        }
        let group = mask
            .as_dict()
            .map_err(|_| EngineFailure::limit_exceeded())?
            .get(b"G")
            .map_err(|_| EngineFailure::limit_exceeded())?;
        let mask_scope = scope.mask_scope();
        self.target(Some(group), &mask_scope, depth, active)
    }
    fn page(&mut self, page_id: ObjectId) -> Result<ScopedStreamFacts, EngineFailure> {
        let page = self
            .doc
            .get_dictionary(page_id)
            .map_err(|_| EngineFailure::limit_exceeded())?;
        let scope = Scope::from_page(self.doc, inherited_resources(self.doc, page_id)?)?;
        let mut roots = Vec::new();
        if let Ok(contents) = page.get(b"Contents") {
            stream_ids(self.doc, contents, false, 0, &mut roots)?;
        }
        let contents_count = roots.len();
        if let Ok(annotations) = page.get(b"Annots") {
            let annotations = resolve(Some(self.doc), annotations)?
                .1
                .as_array()
                .map_err(|_| EngineFailure::limit_exceeded())?;
            for annotation in annotations {
                let annotation = resolve(Some(self.doc), annotation)?
                    .1
                    .as_dict()
                    .map_err(|_| EngineFailure::limit_exceeded())?;
                if let Ok(appearance) = annotation.get(b"AP") {
                    stream_ids(self.doc, appearance, true, 0, &mut roots)?;
                }
            }
        }
        let mut page_cost = ScopedStreamFacts::default();
        for (index, id) in roots.into_iter().enumerate() {
            let stream_cost =
                self.stream_inner(id, &scope, 0, &mut HashSet::new(), index < contents_count)?;
            page_cost = self.add_cost(page_cost, stream_cost)?;
        }
        Ok(page_cost)
    }
}

#[cfg(test)]
pub(crate) fn check(
    ctx: &ToolContext<'_>,
    doc: &lopdf::Document,
    facts: &BTreeMap<ObjectId, Facts>,
    pages: Option<&ariad_core::tools::PageRange>,
    page_contents: &HashSet<ObjectId>,
) -> Result<(), EngineFailure> {
    check_inner(ctx, doc, facts, None, pages, page_contents)
}

pub(crate) fn check_page_with_scoped_ops(
    ctx: &ToolContext<'_>,
    doc: &lopdf::Document,
    page_id: ObjectId,
    facts: &BTreeMap<ObjectId, Facts>,
    scoped_ops: &HashMap<(ObjectId, [u8; 32]), ScopedStreamFacts>,
    page_contents: &HashSet<ObjectId>,
) -> Result<(), EngineFailure> {
    let mut calc = Calculator {
        ctx,
        doc,
        facts,
        scoped_ops: Some(scoped_ops),
        memo: HashMap::new(),
        glyph_memo: HashMap::new(),
        page_contents: page_contents.clone(),
    };
    calc.page(page_id)?;
    Ok(())
}

pub(crate) fn check_with_scoped_ops(
    ctx: &ToolContext<'_>,
    doc: &lopdf::Document,
    facts: &BTreeMap<ObjectId, Facts>,
    scoped_ops: &HashMap<(ObjectId, [u8; 32]), ScopedStreamFacts>,
    pages: Option<&ariad_core::tools::PageRange>,
    page_contents: &HashSet<ObjectId>,
) -> Result<(), EngineFailure> {
    check_inner(ctx, doc, facts, Some(scoped_ops), pages, page_contents)
}

fn check_inner(
    ctx: &ToolContext<'_>,
    doc: &lopdf::Document,
    facts: &BTreeMap<ObjectId, Facts>,
    scoped_ops: Option<&HashMap<(ObjectId, [u8; 32]), ScopedStreamFacts>>,
    pages: Option<&ariad_core::tools::PageRange>,
    page_contents: &HashSet<ObjectId>,
) -> Result<(), EngineFailure> {
    let mut calc = Calculator {
        ctx,
        doc,
        facts,
        scoped_ops,
        memo: HashMap::new(),
        glyph_memo: HashMap::new(),
        page_contents: page_contents.clone(),
    };
    let mut total = 0usize;
    let all_pages = preflight_page_tree(doc, usize::from(ctx.limits().max_nesting_depth))?;
    let selected = match pages {
        Some(range) => ctx.resolve_pages(range, all_pages.len())?,
        None => (1..=all_pages.len()).collect(),
    };
    for number in selected {
        let page_id = all_pages[number - 1];
        let page_cost = calc.page(page_id)?;
        total = limit(
            total
                .checked_add(page_cost.ops)
                .ok_or_else(EngineFailure::limit_exceeded)?,
            MAX_DOCUMENT_WORK,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariad_core::limits::Limits;
    use ariad_engine_io::RunToolPaths;
    use lopdf::dictionary;

    fn context<'a>(dir: &std::path::Path, sink: &'a mut Vec<u8>) -> ToolContext<'a> {
        let paths = RunToolPaths {
            workspace: dir.to_path_buf(),
            inputs: vec![],
            output_dir: dir.to_path_buf(),
            work_dir: dir.to_path_buf(),
        };
        ToolContext::new("budget", "pdf.render", paths, Limits::local(), sink)
    }
    fn facts(ops: usize, calls: &[(&[u8], &[u8])]) -> Facts {
        Facts {
            ops,
            glyphs: 0,
            inline_pixels: 0,
            inline_bytes: 0,
            calls: calls
                .iter()
                .map(|(op, name)| Call {
                    op: op.to_vec(),
                    name: name.to_vec(),
                })
                .collect(),
        }
    }
    fn form(doc: &mut lopdf::Document, dict: Dictionary) -> ObjectId {
        doc.add_object(lopdf::Stream::new(dict, b"q Q".to_vec()))
    }
    #[test]
    fn page_content_uses_page_resources() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = Vec::new();
        let ctx = context(dir.path(), &mut sink);
        let mut doc = lopdf::Document::new();
        let pattern = form(&mut doc, dictionary! { "PatternType" => 1 });
        let root = form(
            &mut doc,
            dictionary! { "Resources" => dictionary! { "Pattern" => dictionary! {} } },
        );
        let resources = Object::Dictionary(
            dictionary! { "Pattern" => dictionary! { "P" => Object::Reference(pattern) } },
        );
        let scope = Scope::from_page(&doc, Some(&resources)).unwrap();
        let facts = BTreeMap::from([
            (root, facts(1, &[(b"scn", b"P")])),
            (pattern, facts(17, &[])),
        ]);
        let mut calc = Calculator {
            ctx: &ctx,
            doc: &doc,
            facts: &facts,
            scoped_ops: None,
            memo: HashMap::new(),
            glyph_memo: HashMap::new(),
            page_contents: HashSet::new(),
        };
        assert_eq!(
            calc.stream_inner(root, &scope, 0, &mut HashSet::new(), true)
                .unwrap()
                .ops,
            18
        );
        assert_eq!(
            calc.stream(root, &scope, 0, &mut HashSet::new())
                .unwrap()
                .ops,
            1
        );
        calc.page_contents.insert(root);
        assert_eq!(
            calc.stream(root, &scope, 0, &mut HashSet::new())
                .unwrap()
                .ops,
            18
        );
    }
    #[test]
    fn inherited_font_keeps_its_selection_resources() {
        let doc = lopdf::Document::new();
        let parent = Object::Dictionary(dictionary! {
            "Font" => dictionary! { "F" => dictionary! { "Subtype" => "Type3" } },
            "Pattern" => dictionary! { "P" => 7 },
        });
        let parent = Scope::from_page(&doc, Some(&parent)).unwrap();
        let child = Object::Dictionary(
            dictionary! { "Font" => dictionary! {}, "Pattern" => dictionary! {} },
        );
        let child = parent.child(&doc, Some(&child)).unwrap();
        assert!(child.get(b"Pattern", b"P").is_none());
        assert!(child.inherited_fonts[0].1[b"Pattern".as_slice()].contains_key(b"P".as_slice()));
    }
    #[test]
    fn pattern_and_soft_mask_costs_multiply_through_forms() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = Vec::new();
        let ctx = context(dir.path(), &mut sink);
        let mut doc = lopdf::Document::new();
        let child = form(&mut doc, dictionary! { "Subtype" => "Form" });
        let pattern = form(
            &mut doc,
            dictionary! { "PatternType" => 1, "Resources" => dictionary! { "XObject" => dictionary! { "F" => Object::Reference(child) } } },
        );
        let root = form(&mut doc, dictionary! {});
        let mut scope = Scope::default();
        Rc::make_mut(&mut scope.current).insert(
            b"Pattern".to_vec(),
            Rc::new(Category::from(BTreeMap::from([(
                b"P".to_vec(),
                Object::Reference(pattern),
            )]))),
        );
        Rc::make_mut(&mut scope.current).insert(
            b"ExtGState".to_vec(),
            Rc::new(Category::from(BTreeMap::from([(
                b"S".to_vec(),
                Object::Dictionary(
                    dictionary! { "SMask" => dictionary! { "G" => Object::Reference(child) } },
                ),
            )]))),
        );
        scope.page = scope.current.clone();
        scope.physical_page = scope.page.clone();
        let facts = BTreeMap::from([
            (
                root,
                facts(3, &[(b"scn", b"P"), (b"scn", b"P"), (b"gs", b"S")]),
            ),
            (pattern, facts(5, &[(b"Do", b"F")])),
            (child, facts(2, &[])),
        ]);
        let mut calc = Calculator {
            ctx: &ctx,
            doc: &doc,
            facts: &facts,
            scoped_ops: None,
            memo: HashMap::new(),
            glyph_memo: HashMap::new(),
            page_contents: HashSet::new(),
        };
        assert_eq!(
            calc.stream(root, &scope, 0, &mut HashSet::new())
                .unwrap()
                .ops,
            19
        );
    }
    #[test]
    fn soft_mask_group_resolves_names_from_page_resources() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = Vec::new();
        let ctx = context(dir.path(), &mut sink);
        let mut doc = lopdf::Document::new();
        let expensive = form(&mut doc, dictionary! { "Subtype" => "Form" });
        let cheap = form(&mut doc, dictionary! { "Subtype" => "Form" });
        let group = form(&mut doc, dictionary! { "Subtype" => "Form" });
        let gs = Object::Dictionary(
            dictionary! { "SMask" => dictionary! { "G" => Object::Reference(group) } },
        );
        let mut scope = Scope::default();
        Rc::make_mut(&mut scope.page).insert(
            b"XObject".to_vec(),
            Rc::new(Category::from(BTreeMap::from([(
                b"F".to_vec(),
                Object::Reference(expensive),
            )]))),
        );
        Rc::make_mut(&mut scope.current).insert(
            b"XObject".to_vec(),
            Rc::new(Category::from(BTreeMap::from([(
                b"F".to_vec(),
                Object::Reference(cheap),
            )]))),
        );
        scope.physical_page = scope.page.clone();
        // A glyph may replace its fallback without changing the actual page.
        scope.page = scope.current.clone();
        let facts = BTreeMap::from([
            (expensive, facts(500, &[])),
            (cheap, facts(1, &[])),
            (group, facts(2, &[(b"Do", b"F")])),
        ]);
        let mut calc = Calculator {
            ctx: &ctx,
            doc: &doc,
            facts: &facts,
            scoped_ops: None,
            memo: HashMap::new(),
            glyph_memo: HashMap::new(),
            page_contents: HashSet::new(),
        };
        assert_eq!(
            calc.mask(Some(&gs), &scope, 0, &mut HashSet::new())
                .unwrap()
                .ops,
            502
        );
    }
    #[test]
    fn many_type3_fonts_and_glyphs_are_memoized() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = Vec::new();
        let ctx = context(dir.path(), &mut sink);
        let mut doc = lopdf::Document::new();
        let mut all = BTreeMap::new();
        let mut procs = Dictionary::new();
        for i in 0..64 {
            let id = form(&mut doc, dictionary! {});
            procs.set(format!("G{i}"), Object::Reference(id));
            all.insert(id, facts(5, &[]));
        }
        let mut fonts = Dictionary::new();
        for i in 0..64 {
            let id =
                doc.add_object(dictionary! { "Subtype" => "Type3", "CharProcs" => procs.clone() });
            fonts.set(format!("F{i}"), Object::Reference(id));
        }
        let child = form(&mut doc, dictionary! { "Subtype" => "Form" });
        let root = form(&mut doc, dictionary! {});
        let resources = Object::Dictionary(
            dictionary! { "Font" => fonts, "XObject" => dictionary! { "F" => Object::Reference(child) } },
        );
        let scope = Scope::from_page(&doc, Some(&resources)).unwrap();
        all.insert(
            child,
            Facts {
                ops: 1,
                glyphs: 20,
                ..Facts::default()
            },
        );
        all.insert(
            root,
            facts(1, &vec![(b"Do".as_slice(), b"F".as_slice()); 1000]),
        );
        let mut calc = Calculator {
            ctx: &ctx,
            doc: &doc,
            facts: &all,
            scoped_ops: None,
            memo: HashMap::new(),
            glyph_memo: HashMap::new(),
            page_contents: HashSet::new(),
        };
        let start = std::time::Instant::now();
        let err = calc
            .stream(root, &scope, 0, &mut HashSet::new())
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::LimitExceeded);
        assert!(err.message.contains("Content execution cost"), "{err:?}");
        assert!(
            start.elapsed() < std::time::Duration::from_secs(1),
            "elapsed: {:?}",
            start.elapsed()
        );
    }
    #[test]
    fn appearance_is_charged_for_each_annotation() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = Vec::new();
        let ctx = context(dir.path(), &mut sink);
        let mut doc = lopdf::Document::new();
        let pages = doc.new_object_id();
        let page = doc.new_object_id();
        let content = form(&mut doc, dictionary! {});
        let appearance = form(&mut doc, dictionary! { "Subtype" => "Form" });
        let annotation = doc.add_object(dictionary! { "Type" => "Annot", "Subtype" => "Stamp", "AP" => dictionary! { "N" => Object::Reference(appearance) } });
        doc.objects.insert(page, Object::Dictionary(dictionary! { "Type" => "Page", "Parent" => Object::Reference(pages), "Contents" => Object::Reference(content), "Annots" => vec![Object::Reference(annotation), Object::Reference(annotation)] }));
        doc.objects.insert(pages, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1 }));
        let catalog = doc
            .add_object(dictionary! { "Type" => "Catalog", "Pages" => Object::Reference(pages) });
        doc.trailer.set("Root", Object::Reference(catalog));
        let all = BTreeMap::from([(content, facts(1, &[])), (appearance, facts(60_000, &[]))]);
        let error = check(&ctx, &doc, &all, None, &HashSet::new()).unwrap_err();
        assert!(error.message.contains("120001"));
    }

    #[test]
    fn content_nesting_is_limited_to_eight() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = Vec::new();
        let ctx = context(dir.path(), &mut sink);
        let run = |wraps: usize| {
            let mut doc = lopdf::Document::new();
            let mut child = form(&mut doc, dictionary! { "Subtype" => "Form" });
            let mut all = BTreeMap::from([(child, facts(1, &[]))]);
            for _ in 0..wraps {
                child = form(
                    &mut doc,
                    dictionary! { "Subtype" => "Form", "Resources" => dictionary! { "XObject" => dictionary! { "F" => Object::Reference(child) } } },
                );
                all.insert(child, facts(1, &[(b"Do", b"F")]));
            }
            let mut calc = Calculator {
                ctx: &ctx,
                doc: &doc,
                facts: &all,
                scoped_ops: None,
                memo: HashMap::new(),
                glyph_memo: HashMap::new(),
                page_contents: HashSet::new(),
            };
            calc.stream(child, &Scope::default(), 0, &mut HashSet::new())
        };
        // Eight nested invocations reach depth eight and stay within the limit.
        assert_eq!(run(MAX_CONTENT_NESTING_DEPTH).unwrap().ops, 9);
        // One more level is refused by the depth check, not by the cost budget.
        let err = run(MAX_CONTENT_NESTING_DEPTH + 1).unwrap_err();
        assert_eq!(err.code, ErrorCode::LimitExceeded);
        assert_eq!(err.message, "A resource limit was exceeded.");
    }
    #[test]
    fn form_resources_without_category_fall_back_to_page_not_parent() {
        // PDFium `CPDF_StreamContentParser::FindResourceHolder` looks up the
        // form's own category, then the page's; the invoking form is skipped.
        let dir = tempfile::tempdir().unwrap();
        let mut sink = Vec::new();
        let ctx = context(dir.path(), &mut sink);
        let run = |inner_resources: Option<Dictionary>| {
            let mut doc = lopdf::Document::new();
            let expensive = form(&mut doc, dictionary! { "Subtype" => "Form" });
            let cheap = form(&mut doc, dictionary! { "Subtype" => "Form" });
            let mut inner_dict = dictionary! { "Subtype" => "Form" };
            if let Some(resources) = inner_resources {
                inner_dict.set("Resources", resources);
            }
            let inner = form(&mut doc, inner_dict);
            let outer = form(
                &mut doc,
                dictionary! {
                    "Subtype" => "Form",
                    "Resources" => dictionary! { "XObject" => dictionary! {
                        "X" => Object::Reference(cheap),
                        "H" => Object::Reference(inner),
                    } },
                },
            );
            let page = Object::Dictionary(dictionary! {
                "XObject" => dictionary! { "X" => Object::Reference(expensive) },
            });
            let scope = Scope::from_page(&doc, Some(&page)).unwrap();
            let table = BTreeMap::from([
                (outer, facts(1, &[(b"Do", b"H")])),
                (inner, facts(1, &[(b"Do", b"X"), (b"Do", b"X")])),
                (expensive, facts(50_000, &[])),
                (cheap, facts(1, &[])),
            ]);
            let mut calc = Calculator {
                ctx: &ctx,
                doc: &doc,
                facts: &table,
                scoped_ops: None,
                memo: HashMap::new(),
                glyph_memo: HashMap::new(),
                page_contents: HashSet::new(),
            };
            calc.stream(outer, &scope, 0, &mut HashSet::new())
        };
        // Without /Resources the inner form reuses the outer dictionary: cheap /X.
        assert_eq!(run(None).unwrap().ops, 4);
        // /Resources without /XObject resolves /X from the page; the inner form
        // alone costs 1 + 2 * 50_000.
        let err = run(Some(dictionary! { "Font" => dictionary! {} })).unwrap_err();
        assert_eq!(err.code, ErrorCode::LimitExceeded);
        assert_eq!(
            err.message,
            "Content execution cost 100001 exceeds limit 100000."
        );
    }

    #[test]
    fn scoped_ops_missing_entry_fails_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = ariad_engine_io::RunToolPaths {
            workspace: dir.path().to_path_buf(),
            inputs: vec![],
            output_dir: dir.path().to_path_buf(),
            work_dir: dir.path().to_path_buf(),
        };
        let mut sink = Vec::new();
        let ctx = crate::output::ToolContext::new(
            "missing-scoped-ops",
            "pdf.render",
            paths,
            ariad_core::limits::Limits::local(),
            &mut sink,
        );
        let mut doc = lopdf::Document::new();
        let content_id = doc.add_object(lopdf::Stream::new(Dictionary::new(), b"q Q".to_vec()));
        let page_id = doc.add_object(lopdf::dictionary! {
            "Type" => "Page",
            "Contents" => Object::Reference(content_id),
        });
        let pages_id = doc.add_object(lopdf::dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        });
        if let Ok(page_dict) = doc.get_dictionary_mut(page_id) {
            page_dict.set("Parent", Object::Reference(pages_id));
        }
        let catalog_id = doc.add_object(lopdf::dictionary! {
            "Type" => "Catalog",
            "Pages" => Object::Reference(pages_id),
        });
        doc.trailer.set("Root", Object::Reference(catalog_id));

        let mut facts = BTreeMap::new();
        facts.insert(
            content_id,
            Facts {
                ops: 1,
                ..Facts::default()
            },
        );
        let mut page_contents = HashSet::new();
        page_contents.insert(content_id);
        let empty_scoped_ops = HashMap::new();

        let err =
            check_with_scoped_ops(&ctx, &doc, &facts, &empty_scoped_ops, None, &page_contents)
                .expect_err(
                    "Some(scoped_ops) missing entry must fail closed, never fallback to facts.ops",
                );
        assert_eq!(err.code, EngineFailure::limit_exceeded().code);
    }

    use super::object_digest;

    #[test]
    fn object_digest_is_canonical_and_domain_separated() {
        let mut dict_a = Dictionary::new();
        dict_a.set("A", 1i64);
        dict_a.set("B", Object::Real(2.5));
        let mut dict_b = Dictionary::new();
        dict_b.set("B", Object::Real(2.5));
        dict_b.set("A", 1i64);
        assert_eq!(
            object_digest(&Object::Dictionary(dict_a.clone())),
            object_digest(&Object::Dictionary(dict_b))
        );

        let stream_a = Object::Stream(lopdf::Stream::new(dict_a.clone(), b"q Q".to_vec()));
        let stream_b = Object::Stream(lopdf::Stream::new(dict_a.clone(), b"q Q ".to_vec()));
        assert_ne!(
            object_digest(&Object::Dictionary(dict_a)),
            object_digest(&stream_a)
        );
        assert_ne!(object_digest(&stream_a), object_digest(&stream_b));

        let variants = [
            Object::Null,
            Object::Boolean(false),
            Object::Boolean(true),
            Object::Integer(1),
            Object::Real(1.0),
            Object::Real(0.0),
            Object::Real(-0.0),
            Object::Name(b"ab".to_vec()),
            Object::String(b"ab".to_vec(), lopdf::StringFormat::Literal),
            Object::Array(vec![
                Object::Name(b"ab".to_vec()),
                Object::Name(b"c".to_vec()),
            ]),
            Object::Array(vec![
                Object::Name(b"a".to_vec()),
                Object::Name(b"bc".to_vec()),
            ]),
            Object::Reference((1, 0)),
            Object::Reference((2, 0)),
            Object::Reference((1, 1)),
        ];
        let unique: HashSet<[u8; 32]> = variants.iter().map(object_digest).collect();
        assert_eq!(unique.len(), variants.len());
    }

    #[test]
    fn category_and_scope_digests_cache_and_invalidate_on_mutation() {
        let doc = lopdf::Document::new();
        let mut category = Rc::new(Category::from(BTreeMap::from([(
            b"F".to_vec(),
            Object::Reference((10, 0)),
        )])));
        let initial_cat = category.digest();
        assert!(category.digest.get().is_some());

        let cloned_cat = (*category).clone();
        assert!(cloned_cat.digest.get().is_none());
        assert_eq!(cloned_cat.digest(), initial_cat);

        Rc::make_mut(&mut category).insert(b"G".to_vec(), Object::Reference((11, 0)));
        assert!(category.digest.get().is_none());
        assert_ne!(category.digest(), initial_cat);

        let page = Object::Dictionary(dictionary! {
            "Font" => dictionary! { "F0" => Object::Reference((1, 0)) },
            "Pattern" => dictionary! { "P0" => Object::Reference((2, 0)) },
        });
        let parent = Scope::from_page(&doc, Some(&page)).unwrap();
        let parent_digest = parent.digest();
        assert!(parent.current.digest.get().is_some());
        let parent_pattern_rc = Rc::clone(&parent.current[b"Pattern".as_slice()]);
        assert!(parent_pattern_rc.digest.get().is_some());

        let child_res = Object::Dictionary(dictionary! {
            "Font" => dictionary! { "F1" => Object::Reference((3, 0)) },
        });
        let child = parent.child(&doc, Some(&child_res)).unwrap();
        assert!(child.current.digest.get().is_none());
        assert_ne!(child.digest(), parent_digest);
        // Unchanged sibling categories remain cached and shared across child scopes.
        assert!(Rc::ptr_eq(
            &child.current[b"Pattern".as_slice()],
            &parent_pattern_rc
        ));
        assert!(parent_pattern_rc.digest.get().is_some());
    }

    #[test]
    fn inherited_fonts_sharing_large_categories_digest_quickly() {
        let mut doc = lopdf::Document::new();
        let mut fonts = Dictionary::new();
        let mut patterns = Dictionary::new();
        for i in 0..128 {
            let font_id = doc.add_object(dictionary! { "Subtype" => "Type3" });
            fonts.set(format!("F{i}"), Object::Reference(font_id));
            let pat_id = form(&mut doc, dictionary! { "PatternType" => 1 });
            patterns.set(format!("P{i}"), Object::Reference(pat_id));
        }
        let parent_res = Object::Dictionary(dictionary! {
            "Font" => fonts,
            "Pattern" => patterns,
        });
        let parent = Scope::from_page(&doc, Some(&parent_res)).unwrap();
        let child_res = Object::Dictionary(dictionary! {
            "XObject" => dictionary! { "X0" => Object::Reference((999, 0)) },
        });
        let start = std::time::Instant::now();
        let child = parent.child(&doc, Some(&child_res)).unwrap();
        let digest = child.digest();
        assert_eq!(child.inherited_fonts.len(), 128);
        assert_ne!(digest, parent.digest());
        assert!(
            start.elapsed() < std::time::Duration::from_secs(1),
            "elapsed: {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn scoped_ops_adjusts_cost_per_colorspace_scope() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut doc = lopdf::Document::new();
        let form_id = form(&mut doc, dictionary! { "Subtype" => "Form" });
        let page1_stream = form(&mut doc, dictionary! {});
        let page2_stream = form(&mut doc, dictionary! {});
        let pages_id = doc.new_object_id();
        let p1_res = Object::Dictionary(dictionary! {
            "XObject" => dictionary! { "F" => Object::Reference(form_id) },
            "ColorSpace" => dictionary! { "Cs" => "DeviceGray" },
        });
        let p2_res = Object::Dictionary(dictionary! {
            "XObject" => dictionary! { "F" => Object::Reference(form_id) },
            "ColorSpace" => dictionary! { "Cs" => "DeviceRGB" },
        });
        let page1_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => Object::Reference(pages_id),
            "Resources" => p1_res.clone(),
            "Contents" => Object::Reference(page1_stream),
        });
        let page2_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => Object::Reference(pages_id),
            "Resources" => p2_res.clone(),
            "Contents" => Object::Reference(page2_stream),
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page1_id), Object::Reference(page2_id)],
                "Count" => 2,
            }),
        );
        let root_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => Object::Reference(pages_id),
        });
        doc.trailer.set("Root", Object::Reference(root_id));

        let scope1 = Scope::from_page(&doc, Some(&p1_res)).unwrap();
        let scope2 = Scope::from_page(&doc, Some(&p2_res)).unwrap();
        assert_ne!(scope1.colorspace_digest(), scope2.colorspace_digest());

        let page_contents = HashSet::from([page1_stream, page2_stream]);
        let table = BTreeMap::from([
            (page1_stream, facts(20_000, &[(b"Do", b"F")])),
            (page2_stream, facts(11_000, &[(b"Do", b"F")])),
            (form_id, facts(30_000, &[])),
        ]);
        let scoped_ops = HashMap::from([
            (
                (page1_stream, scope1.colorspace_digest()),
                ScopedStreamFacts {
                    ops: 20_000,
                    inline_pixels: 0,
                    inline_bytes: 0,
                },
            ),
            (
                (page2_stream, scope2.colorspace_digest()),
                ScopedStreamFacts {
                    ops: 11_000,
                    inline_pixels: 0,
                    inline_bytes: 0,
                },
            ),
            (
                (form_id, scope1.colorspace_digest()),
                ScopedStreamFacts {
                    ops: 30_000,
                    inline_pixels: 0,
                    inline_bytes: 0,
                },
            ),
            (
                (form_id, scope2.colorspace_digest()),
                ScopedStreamFacts {
                    ops: 90_000,
                    inline_pixels: 0,
                    inline_bytes: 0,
                },
            ),
        ]);

        let mut sink = Vec::new();
        let ctx = context(dir.path(), &mut sink);
        check_with_scoped_ops(
            &ctx,
            &doc,
            &table,
            &scoped_ops,
            Some(&ariad_core::tools::PageRange::parse("1").unwrap()),
            &page_contents,
        )
        .expect("Page 1 scoped cost (20_000 + 30_000) fits");
        let err = check_with_scoped_ops(&ctx, &doc, &table, &scoped_ops, None, &page_contents)
            .expect_err("Page 2 scoped cost (11_000 + 90_000) must exceed 100_000");
        assert!(
            err.message.contains("Content execution cost"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn soft_mask_group_amplifies_inline_pixels_and_bytes_from_physical_page() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut sink = Vec::new();
        let mut limits = Limits::local();
        limits.max_asset_bytes = Some(400);
        let paths = RunToolPaths {
            workspace: dir.path().to_path_buf(),
            inputs: vec![],
            output_dir: dir.path().to_path_buf(),
            work_dir: dir.path().to_path_buf(),
        };
        let ctx = ToolContext::new("smask-inline", "pdf.render", paths, limits, &mut sink);

        let mut doc = lopdf::Document::new();
        let phys_target = form(&mut doc, dictionary! { "Subtype" => "Form" });
        let shadowed_target = form(&mut doc, dictionary! { "Subtype" => "Form" });
        let group = form(&mut doc, dictionary! { "Subtype" => "Form" });
        let outer = form(
            &mut doc,
            dictionary! {
                "Subtype" => "Form",
                "Resources" => dictionary! {
                    "XObject" => dictionary! { "ImForm" => Object::Reference(shadowed_target) },
                    "ExtGState" => dictionary! {
                        "GS" => dictionary! { "SMask" => dictionary! { "G" => Object::Reference(group) } }
                    }
                }
            },
        );
        let content = form(&mut doc, dictionary! {});
        let pages_id = doc.new_object_id();
        let page_res = Object::Dictionary(dictionary! {
            "XObject" => dictionary! {
                "Outer" => Object::Reference(outer),
                "ImForm" => Object::Reference(phys_target),
            },
        });
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => Object::Reference(pages_id),
            "Resources" => page_res,
            "Contents" => Object::Reference(content),
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => Object::Reference(pages_id),
        });
        doc.trailer.set("Root", Object::Reference(catalog_id));

        let table = BTreeMap::from([
            (content, facts(1, &[(b"Do", b"Outer")])),
            // 5 * 30 = 150 pixels (> max_pixels = 100), whereas shadowed_target only has 1 pixel
            (
                outer,
                facts(
                    1,
                    &[
                        (b"gs", b"GS"),
                        (b"gs", b"GS"),
                        (b"gs", b"GS"),
                        (b"gs", b"GS"),
                        (b"gs", b"GS"),
                    ],
                ),
            ),
            (group, facts(1, &[(b"Do", b"ImForm")])),
            (
                phys_target,
                Facts {
                    ops: 1,
                    glyphs: 0,
                    inline_pixels: 30,
                    inline_bytes: 90,
                    calls: Vec::new(),
                },
            ),
            (
                shadowed_target,
                Facts {
                    ops: 1,
                    glyphs: 0,
                    inline_pixels: 1,
                    inline_bytes: 1,
                    calls: Vec::new(),
                },
            ),
        ]);
        let page_contents = HashSet::from([content]);
        let err = check(&ctx, &doc, &table, None, &page_contents)
            .expect_err("SMask /G inline pixel amplification must exceed max_pixels");
        assert_eq!(err.code, ErrorCode::LimitExceeded);
        assert_eq!(err.message, "Page inline image totals exceed the limit.");
    }

    #[test]
    fn type3_resources_omitting_xobject_fall_back_to_invoking_scope() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut sink = Vec::new();
        let ctx = context(dir.path(), &mut sink);
        let mut doc = lopdf::Document::new();
        let page_form = form(&mut doc, dictionary! { "Subtype" => "Form" });
        let char_proc = form(&mut doc, dictionary! {});
        // Type3 font has /Resources present, omitting /XObject -> must fall back to selected scope's /XObject.
        let type3_font = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type3",
            "CharProcs" => dictionary! { "A" => Object::Reference(char_proc) },
            "Resources" => dictionary! { "Pattern" => dictionary! {} },
        });
        let content = form(&mut doc, dictionary! {});
        let pages_id = doc.new_object_id();
        let page_res = Object::Dictionary(dictionary! {
            "Font" => dictionary! { "T" => Object::Reference(type3_font) },
            "XObject" => dictionary! { "F" => Object::Reference(page_form) },
        });
        let scope = Scope::from_page(&doc, Some(&page_res)).unwrap();
        let glyph_scope = scope
            .type3_glyph_scope(
                &doc,
                scope.current(),
                doc.get_dictionary(type3_font).unwrap(),
            )
            .unwrap();
        assert_eq!(glyph_scope.xobject_id(&doc, b"F").unwrap(), Some(page_form));

        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => Object::Reference(pages_id),
            "Resources" => page_res,
            "Contents" => Object::Reference(content),
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => Object::Reference(pages_id),
        });
        doc.trailer.set("Root", Object::Reference(catalog_id));

        let table = BTreeMap::from([
            (
                content,
                Facts {
                    ops: 1,
                    glyphs: 20,
                    inline_pixels: 0,
                    inline_bytes: 0,
                    calls: Vec::new(),
                },
            ),
            (char_proc, facts(1, &[(b"Do", b"F")])),
            (page_form, facts(6_000, &[])),
        ]);
        let err = check(&ctx, &doc, &table, None, &HashSet::from([content]))
            .expect_err("20 glyphs * 6_000 ops must exceed 100_000 via inherited XObject");
        assert_eq!(err.code, ErrorCode::LimitExceeded);
        assert!(err.message.contains("Content execution cost"), "{err:?}");
    }
}

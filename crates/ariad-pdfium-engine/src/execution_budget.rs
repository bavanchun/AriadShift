//! Bound execution of content resources before PDFium opens a document.
use std::cell::OnceCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::rc::Rc;

use ariad_core::protocol::ErrorCode;
use ariad_engine_io::EngineFailure;
use lopdf::{Dictionary, Object, ObjectId};
use sha2::{Digest, Sha256};

use crate::{
    extract_images::{preflight_page_tree, resolve},
    output::ToolContext,
};

#[derive(Debug)]
pub(crate) struct Call {
    pub op: Vec<u8>,
    pub name: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct Facts {
    pub ops: usize,
    pub glyphs: usize,
    pub calls: Vec<Call>,
}

type Categories = BTreeMap<Vec<u8>, Rc<BTreeMap<Vec<u8>, Object>>>;

struct ScopeHasher(Sha256);

impl std::fmt::Write for ScopeHasher {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        self.0.update(value.as_bytes());
        Ok(())
    }
}

#[derive(Clone, Default)]
struct Scope {
    page: Rc<Categories>,
    physical_page: Rc<Categories>,
    current: Rc<Categories>,
    inherited_fonts: Vec<(Object, Rc<Categories>)>,
    digest: OnceCell<[u8; 32]>,
}

impl Scope {
    fn from_page(doc: &lopdf::Document, resources: Option<&Object>) -> Result<Self, EngineFailure> {
        let current = Rc::new(categories(doc, resources)?);
        Ok(Self {
            page: current.clone(),
            physical_page: current.clone(),
            current,
            inherited_fonts: Vec::new(),
            digest: OnceCell::new(),
        })
    }
    fn child(
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
    fn digest(&self) -> [u8; 32] {
        *self.digest.get_or_init(|| {
            let mut hash = ScopeHasher(Sha256::new());
            write!(
                hash,
                "{:?}|{:?}|{:?}|{:?}",
                self.page, self.physical_page, self.current, self.inherited_fonts
            )
            .expect("hash writer cannot fail");
            hash.0.finalize().into()
        })
    }
    fn get(&self, category: &[u8], name: &[u8]) -> Option<&Object> {
        self.current
            .get(category)
            .and_then(|values| values.get(name))
    }
}

fn categories(
    doc: &lopdf::Document,
    resources: Option<&Object>,
) -> Result<Categories, EngineFailure> {
    let mut out = Categories::new();
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

fn inherited_resources(
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

fn stream_ids(
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

type MemoKey = (ObjectId, [u8; 32], usize, Vec<ObjectId>, bool);
type GlyphKey = ([u8; 32], usize, Vec<ObjectId>);

struct Calculator<'a, 'b> {
    ctx: &'a ToolContext<'b>,
    doc: &'a lopdf::Document,
    facts: &'a BTreeMap<ObjectId, Facts>,
    memo: HashMap<MemoKey, usize>,
    glyph_memo: HashMap<GlyphKey, usize>,
}

impl Calculator<'_, '_> {
    fn stream(
        &mut self,
        id: ObjectId,
        scope: &Scope,
        depth: usize,
        active: &mut HashSet<ObjectId>,
    ) -> Result<usize, EngineFailure> {
        self.stream_inner(id, scope, depth, active, false)
    }
    fn stream_inner(
        &mut self,
        id: ObjectId,
        scope: &Scope,
        depth: usize,
        active: &mut HashSet<ObjectId>,
        page_contents: bool,
    ) -> Result<usize, EngineFailure> {
        self.ctx.check()?;
        if depth > 8 || !active.insert(id) {
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
        let mut cost = facts.ops;
        for call in &facts.calls {
            let extra = match call.op.as_slice() {
                b"Do" => self.target(scope.get(b"XObject", &call.name), &scope, depth, active)?,
                b"scn" | b"SCN" => {
                    self.pattern(scope.get(b"Pattern", &call.name), &scope, depth, active)?
                }
                b"gs" => self.mask(scope.get(b"ExtGState", &call.name), &scope, depth, active)?,
                // The shading operator itself is already included in `facts.ops`.
                b"sh" => 0,
                _ => 0,
            };
            cost = limit(
                cost.checked_add(extra)
                    .ok_or_else(EngineFailure::limit_exceeded)?,
                100_000,
            )?;
        }
        if facts.glyphs > 0 {
            let mut ancestors: Vec<_> = active.iter().copied().collect();
            ancestors.sort_unstable();
            let glyph_key = (scope.digest(), depth, ancestors);
            let max_glyph = if let Some(&cost) = self.glyph_memo.get(&glyph_key) {
                cost
            } else {
                let mut max_glyph = 0usize;
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
                    let selected_scope = Scope {
                        page: selected_resources.clone(),
                        physical_page: scope.physical_page.clone(),
                        current: selected_resources.clone(),
                        inherited_fonts: scope.inherited_fonts.clone(),
                        digest: OnceCell::new(),
                    };
                    let mut glyph_scope =
                        selected_scope.child(self.doc, font.get(b"Resources").ok())?;
                    // PDFium treats even an empty font Resources dictionary as present.
                    // Its selected font scope becomes the CharProc's fallback scope.
                    glyph_scope.page = glyph_scope.current.clone();
                    glyph_scope.digest = OnceCell::new();
                    for (_, proc) in procs.iter() {
                        let (proc_id, _) = resolve(Some(self.doc), proc)?;
                        let glyph_cost = self.stream(
                            proc_id.ok_or_else(EngineFailure::limit_exceeded)?,
                            &glyph_scope,
                            depth + 1,
                            active,
                        )?;
                        max_glyph = max_glyph.max(glyph_cost);
                    }
                }
                self.glyph_memo.insert(glyph_key, max_glyph);
                max_glyph
            };
            cost = limit(
                cost.checked_add(
                    facts
                        .glyphs
                        .checked_mul(max_glyph)
                        .ok_or_else(EngineFailure::limit_exceeded)?,
                )
                .ok_or_else(EngineFailure::limit_exceeded)?,
                100_000,
            )?;
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
    ) -> Result<usize, EngineFailure> {
        let Some(value) = value else {
            return Ok(0);
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
            Some(b"Image") => Ok(0),
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
    ) -> Result<usize, EngineFailure> {
        let Some(value) = value else {
            return Ok(0);
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
            Some(2) => Ok(1),
            _ => Err(EngineFailure::limit_exceeded()),
        }
    }
    fn mask(
        &mut self,
        value: Option<&Object>,
        scope: &Scope,
        depth: usize,
        active: &mut HashSet<ObjectId>,
    ) -> Result<usize, EngineFailure> {
        let Some(value) = value else {
            return Ok(0);
        };
        let dict = resolve(Some(self.doc), value)?
            .1
            .as_dict()
            .map_err(|_| EngineFailure::limit_exceeded())?;
        let Ok(mask) = dict.get(b"SMask") else {
            return Ok(0);
        };
        let mask = resolve(Some(self.doc), mask)?.1;
        if matches!(mask, Object::Name(name) if name == b"None") {
            return Ok(0);
        }
        let group = mask
            .as_dict()
            .map_err(|_| EngineFailure::limit_exceeded())?
            .get(b"G")
            .map_err(|_| EngineFailure::limit_exceeded())?;
        let mask_scope = Scope {
            page: scope.physical_page.clone(),
            physical_page: scope.physical_page.clone(),
            current: scope.physical_page.clone(),
            inherited_fonts: Vec::new(),
            digest: OnceCell::new(),
        };
        self.target(Some(group), &mask_scope, depth, active)
    }
}

pub(crate) fn check(
    ctx: &ToolContext<'_>,
    doc: &lopdf::Document,
    facts: &BTreeMap<ObjectId, Facts>,
    pages: Option<&ariad_core::tools::PageRange>,
) -> Result<(), EngineFailure> {
    let mut calc = Calculator {
        ctx,
        doc,
        facts,
        memo: HashMap::new(),
        glyph_memo: HashMap::new(),
    };
    let mut total = 0usize;
    let all_pages = preflight_page_tree(doc, usize::from(ctx.limits().max_nesting_depth))?;
    let selected = match pages {
        Some(range) => ctx.resolve_pages(range, all_pages.len())?,
        None => (1..=all_pages.len()).collect(),
    };
    for number in selected {
        let page_id = all_pages[number - 1];
        let page = doc
            .get_dictionary(page_id)
            .map_err(|_| EngineFailure::limit_exceeded())?;
        let scope = Scope::from_page(doc, inherited_resources(doc, page_id)?)?;
        let mut roots = Vec::new();
        if let Ok(contents) = page.get(b"Contents") {
            stream_ids(doc, contents, false, 0, &mut roots)?;
        }
        let contents_count = roots.len();
        if let Ok(annotations) = page.get(b"Annots") {
            let annotations = resolve(Some(doc), annotations)?
                .1
                .as_array()
                .map_err(|_| EngineFailure::limit_exceeded())?;
            for annotation in annotations {
                let annotation = resolve(Some(doc), annotation)?
                    .1
                    .as_dict()
                    .map_err(|_| EngineFailure::limit_exceeded())?;
                if let Ok(appearance) = annotation.get(b"AP") {
                    stream_ids(doc, appearance, true, 0, &mut roots)?;
                }
            }
        }
        let mut page_cost = 0usize;
        for (index, id) in roots.into_iter().enumerate() {
            page_cost = limit(
                page_cost
                    .checked_add(calc.stream_inner(
                        id,
                        &scope,
                        0,
                        &mut HashSet::new(),
                        index < contents_count,
                    )?)
                    .ok_or_else(EngineFailure::limit_exceeded)?,
                100_000,
            )?;
        }
        total = limit(
            total
                .checked_add(page_cost)
                .ok_or_else(EngineFailure::limit_exceeded)?,
            1_000_000,
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
            memo: HashMap::new(),
            glyph_memo: HashMap::new(),
        };
        assert_eq!(
            calc.stream_inner(root, &scope, 0, &mut HashSet::new(), true)
                .unwrap(),
            18
        );
        assert_eq!(
            calc.stream(root, &scope, 0, &mut HashSet::new()).unwrap(),
            1
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
            Rc::new(BTreeMap::from([(
                b"P".to_vec(),
                Object::Reference(pattern),
            )])),
        );
        Rc::make_mut(&mut scope.current).insert(
            b"ExtGState".to_vec(),
            Rc::new(BTreeMap::from([(
                b"S".to_vec(),
                Object::Dictionary(
                    dictionary! { "SMask" => dictionary! { "G" => Object::Reference(child) } },
                ),
            )])),
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
            memo: HashMap::new(),
            glyph_memo: HashMap::new(),
        };
        assert_eq!(
            calc.stream(root, &scope, 0, &mut HashSet::new()).unwrap(),
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
            Rc::new(BTreeMap::from([(
                b"F".to_vec(),
                Object::Reference(expensive),
            )])),
        );
        Rc::make_mut(&mut scope.current).insert(
            b"XObject".to_vec(),
            Rc::new(BTreeMap::from([(b"F".to_vec(), Object::Reference(cheap))])),
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
            memo: HashMap::new(),
            glyph_memo: HashMap::new(),
        };
        assert_eq!(
            calc.mask(Some(&gs), &scope, 0, &mut HashSet::new())
                .unwrap(),
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
                calls: Vec::new(),
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
            memo: HashMap::new(),
            glyph_memo: HashMap::new(),
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
        let error = check(&ctx, &doc, &all, None).unwrap_err();
        assert!(error.message.contains("120001"));
    }

    #[test]
    fn content_nesting_is_limited_to_eight() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = Vec::new();
        let ctx = context(dir.path(), &mut sink);
        let mut doc = lopdf::Document::new();
        let mut child = form(&mut doc, dictionary! { "Subtype" => "Form" });
        let mut all = BTreeMap::from([(child, facts(1, &[]))]);
        for _ in 0..9 {
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
            memo: HashMap::new(),
            glyph_memo: HashMap::new(),
        };
        assert!(
            calc.stream(child, &Scope::default(), 0, &mut HashSet::new())
                .is_err()
        );
    }
}

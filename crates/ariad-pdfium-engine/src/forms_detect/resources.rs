//! Inspect form XObject resource graphs with explicit cycle and depth limits.

use crate::output::ToolContext;
use ariad_engine_io::EngineFailure;
use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::HashSet;

struct Walker<'a, 'b, 'c> {
    doc: &'a Document,
    ctx: &'b ToolContext<'c>,
    active: HashSet<ObjectId>,
    visited: HashSet<ObjectId>,
    count: usize,
    cycle: bool,
    truncated: bool,
}
impl Walker<'_, '_, '_> {
    fn dictionary<'a>(&'a self, value: &'a Object) -> Option<&'a Dictionary> {
        let mut current = value;
        let mut seen = HashSet::new();
        while let Object::Reference(id) = current {
            if !seen.insert(*id) || seen.len() > 64 {
                return None;
            }
            current = self.doc.get_object(*id).ok()?;
        }
        match current {
            Object::Dictionary(dict) => Some(dict),
            Object::Stream(stream) => Some(&stream.dict),
            _ => None,
        }
    }
    fn resources(&mut self, resources: &Object, depth: usize) -> Result<(), EngineFailure> {
        if depth > usize::from(self.ctx.limits().max_nesting_depth) {
            return Err(EngineFailure::limit_exceeded());
        }
        if depth > 8 {
            self.truncated = true;
            return Ok(());
        }
        let Some(xobjects) = self
            .dictionary(resources)
            .and_then(|d| d.get(b"XObject").ok())
            .and_then(|o| self.dictionary(o))
        else {
            return Ok(());
        };
        if xobjects.len() > 50_000 || xobjects.len() > self.ctx.limits().max_blocks as usize {
            self.truncated = true;
            return Ok(());
        }
        let values: Vec<Object> = xobjects.iter().map(|(_, v)| v.clone()).collect();
        for value in &values {
            self.count += 1;
            if self.count.is_multiple_of(1024) {
                self.ctx.check()?;
            }
            if self.count > 50_000 || self.count > self.ctx.limits().max_blocks as usize {
                self.truncated = true;
                break;
            }
            let id = value.as_reference().ok();
            if let Some(id) = id {
                if self.active.contains(&id) {
                    self.cycle = true;
                    continue;
                }
                if !self.visited.insert(id) {
                    continue;
                }
                self.active.insert(id);
            }
            let child = self.dictionary(value).and_then(|d| {
                let subtype = d.get(b"Subtype").ok().and_then(|s| s.as_name().ok());
                if subtype == Some(b"Form") {
                    d.get(b"Resources").ok().cloned()
                } else {
                    None
                }
            });
            if let Some(child) = child {
                self.resources(&child, depth + 1)?;
            }
            if let Some(id) = id {
                self.active.remove(&id);
            }
        }
        Ok(())
    }
}
/// Returns whether resource cycles or traversal limits were encountered on a page.
pub(super) fn inspect(
    ctx: &ToolContext<'_>,
    doc: &Document,
    page: ObjectId,
) -> Result<(bool, bool), EngineFailure> {
    let (direct, ids) = doc.get_page_resources(page).map_err(|_| {
        EngineFailure::new(
            ariad_core::protocol::ErrorCode::EngineFailure,
            "Invalid page resources.",
        )
    })?;
    let mut roots: Vec<Object> = direct
        .cloned()
        .map(Object::Dictionary)
        .into_iter()
        .collect();
    roots.extend(ids.into_iter().map(Object::Reference));
    let mut walker = Walker {
        doc,
        ctx,
        active: HashSet::new(),
        visited: HashSet::new(),
        count: 0,
        cycle: false,
        truncated: false,
    };
    for root in &roots {
        walker.resources(root, 0)?;
    }
    Ok((walker.cycle, walker.truncated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariad_core::limits::Limits;
    use ariad_engine_io::RunToolPaths;
    use lopdf::{Stream, dictionary};
    use std::path::PathBuf;

    #[test]
    fn form_resource_cycle_is_detected_without_recursion() {
        let mut doc = Document::with_version("1.5");
        let form = doc.new_object_id();
        let resources = dictionary! {"XObject"=>dictionary!{"Again"=>Object::Reference(form)}};
        doc.objects.insert(
            form,
            Object::Stream(Stream::new(
                dictionary! {"Type"=>"XObject","Subtype"=>"Form","Resources"=>resources.clone()},
                Vec::new(),
            )),
        );
        let page = doc.add_object(dictionary! {"Type"=>"Page","Resources"=>resources});
        let paths = RunToolPaths {
            workspace: PathBuf::new(),
            inputs: vec![PathBuf::from("input.pdf")],
            output_dir: PathBuf::new(),
            work_dir: PathBuf::new(),
        };
        let mut sink = Vec::new();
        let ctx = ToolContext::new(
            "test",
            "pdf.forms_detect",
            paths,
            Limits::local(),
            &mut sink,
        );
        let (cycle, truncated) = inspect(&ctx, &doc, page).unwrap();
        assert!(cycle);
        assert!(!truncated);
    }
}

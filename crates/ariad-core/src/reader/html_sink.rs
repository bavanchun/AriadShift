//! Arena-based DOM TreeSink for html5ever with depth and node limit enforcement.

use std::{
    borrow::Cow,
    cell::{Cell, Ref, RefCell},
    rc::Rc,
};

use html5ever::{
    Attribute, QualName,
    tendril::StrTendril,
    tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink},
};

/// Specific limit condition exceeded while building the DOM tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitExceeded {
    NestingTooDeep { limit: u16 },
    TooManyNodes { limit: usize },
}

/// Index of a node inside the arena.
pub type Handle = usize;

/// Specific data payload for each DOM node.
#[derive(Clone, Debug)]
pub enum NodeData {
    Document,
    Doctype {
        _name: String,
        _public_id: String,
        _system_id: String,
    },
    Text {
        text: String,
    },
    Comment {
        _comment: String,
    },
    Element {
        name: QualName,
        attrs: Vec<Attribute>,
        template_contents: Option<Handle>,
        mathml_annotation_xml_integration_point: bool,
    },
    ProcessingInstruction {
        _target: String,
        _data: String,
    },
}

/// A node in the arena tree.
#[derive(Clone, Debug)]
pub struct DomNode {
    pub parent: Option<Handle>,
    pub children: Vec<Handle>,
    pub data: NodeData,
    pub depth: usize,
    pub max_depth: usize,
}

/// Arena storing all nodes created during parsing.
#[derive(Debug)]
pub struct Arena {
    pub nodes: Vec<DomNode>,
}

impl Arena {
    /// Creates a new arena containing the root document node at handle 0.
    #[must_use]
    pub fn new() -> (Self, Handle) {
        let mut arena = Self { nodes: Vec::new() };
        let doc = arena.alloc(NodeData::Document, 0);
        (arena, doc)
    }

    /// Allocates a new node and returns its handle.
    pub fn alloc(&mut self, data: NodeData, depth: usize) -> Handle {
        let handle = self.nodes.len();
        self.nodes.push(DomNode {
            parent: None,
            children: Vec::new(),
            data,
            depth,
            max_depth: depth,
        });
        handle
    }

    /// Removes a child handle from a parent node's children list, searching from the end.
    pub fn remove_child_from(&mut self, parent: Handle, child: Handle) {
        if let Some(idx) = self.nodes[parent]
            .children
            .iter()
            .rposition(|&h| h == child)
        {
            self.nodes[parent].children.remove(idx);
        }
    }
}

/// `TreeSink` implementation for html5ever with depth and node count monitoring.
pub struct HtmlSink {
    pub arena: RefCell<Arena>,
    pub document_handle: Handle,
    pub limit_hit: Rc<Cell<Option<LimitExceeded>>>,
    pub max_dom_depth: usize,
    pub max_nodes: usize,
}

impl HtmlSink {
    /// Creates a new sink sharing `limit_hit` with the chunk feeder.
    #[must_use]
    pub fn new(
        limit_hit: Rc<Cell<Option<LimitExceeded>>>,
        max_dom_depth: usize,
        max_nodes: usize,
    ) -> Self {
        let (arena, document_handle) = Arena::new();
        Self {
            arena: RefCell::new(arena),
            document_handle,
            limit_hit,
            max_dom_depth,
            max_nodes,
        }
    }

    fn check_node_limit(&self, arena_len: usize) {
        if arena_len > self.max_nodes && self.limit_hit.get().is_none() {
            self.limit_hit.set(Some(LimitExceeded::TooManyNodes {
                limit: self.max_nodes,
            }));
        }
    }

    fn set_node_depth_and_check(&self, arena: &mut Arena, child_handle: Handle, parent: Handle) {
        let is_elem = matches!(arena.nodes[child_handle].data, NodeData::Element { .. });
        let depth = if is_elem {
            arena.nodes[parent].depth + 1
        } else {
            arena.nodes[parent].depth
        };
        let subtree_height = arena.nodes[child_handle]
            .max_depth
            .saturating_sub(arena.nodes[child_handle].depth);
        arena.nodes[child_handle].depth = depth;
        arena.nodes[child_handle].max_depth = depth + subtree_height;

        let mut deepest = arena.nodes[child_handle].max_depth;
        if let NodeData::Element {
            template_contents: Some(tc),
            ..
        } = arena.nodes[child_handle].data
        {
            let tc_height = arena.nodes[tc]
                .max_depth
                .saturating_sub(arena.nodes[tc].depth);
            arena.nodes[tc].depth = depth;
            arena.nodes[tc].max_depth = depth + tc_height;
            deepest = deepest.max(arena.nodes[tc].max_depth);
        }

        if deepest > self.max_dom_depth && self.limit_hit.get().is_none() {
            self.limit_hit.set(Some(LimitExceeded::NestingTooDeep {
                limit: self.max_dom_depth as u16,
            }));
        }

        arena.nodes[parent].max_depth = arena.nodes[parent].max_depth.max(deepest);
    }
}

impl TreeSink for HtmlSink {
    type Handle = Handle;
    type Output = (Arena, Handle);
    type ElemName<'a> = Ref<'a, QualName>;

    fn finish(self) -> Self::Output {
        let handle = self.document_handle;
        (self.arena.into_inner(), handle)
    }

    fn parse_error(&self, _msg: Cow<'static, str>) {}

    fn get_document(&self) -> Self::Handle {
        self.document_handle
    }

    fn elem_name<'a>(&'a self, target: &'a Self::Handle) -> Self::ElemName<'a> {
        Ref::map(self.arena.borrow(), |arena| {
            match &arena.nodes[*target].data {
                NodeData::Element { name, .. } => name,
                _ => panic!("elem_name called on non-element node {target}"),
            }
        })
    }

    fn create_element(
        &self,
        name: QualName,
        attrs: Vec<Attribute>,
        flags: ElementFlags,
    ) -> Self::Handle {
        let mut arena = self.arena.borrow_mut();
        let template_contents = if flags.template {
            Some(arena.alloc(NodeData::Document, 0))
        } else {
            None
        };
        let handle = arena.alloc(
            NodeData::Element {
                name,
                attrs,
                template_contents,
                mathml_annotation_xml_integration_point: flags
                    .mathml_annotation_xml_integration_point,
            },
            0,
        );
        let len = arena.nodes.len();
        drop(arena);
        self.check_node_limit(len);
        handle
    }

    fn create_comment(&self, text: StrTendril) -> Self::Handle {
        let mut arena = self.arena.borrow_mut();
        let handle = arena.alloc(
            NodeData::Comment {
                _comment: text.to_string(),
            },
            0,
        );
        let len = arena.nodes.len();
        drop(arena);
        self.check_node_limit(len);
        handle
    }

    fn create_pi(&self, target: StrTendril, data: StrTendril) -> Self::Handle {
        let mut arena = self.arena.borrow_mut();
        let handle = arena.alloc(
            NodeData::ProcessingInstruction {
                _target: target.to_string(),
                _data: data.to_string(),
            },
            0,
        );
        let len = arena.nodes.len();
        drop(arena);
        self.check_node_limit(len);
        handle
    }

    fn append(&self, parent: &Self::Handle, child: NodeOrText<Self::Handle>) {
        let mut arena = self.arena.borrow_mut();
        match child {
            NodeOrText::AppendText(tendril) => {
                let text_slice: &str = &tendril;
                if text_slice.is_empty() {
                    return;
                }
                if let Some(&last) = arena.nodes[*parent].children.last()
                    && let NodeData::Text { ref mut text } = arena.nodes[last].data
                {
                    text.push_str(text_slice);
                    return;
                }
                let parent_depth = arena.nodes[*parent].depth;
                let child_handle = arena.alloc(
                    NodeData::Text {
                        text: text_slice.to_owned(),
                    },
                    parent_depth,
                );
                arena.nodes[child_handle].parent = Some(*parent);
                arena.nodes[*parent].children.push(child_handle);
                let len = arena.nodes.len();
                drop(arena);
                self.check_node_limit(len);
            }
            NodeOrText::AppendNode(child_handle) => {
                if let Some(old_parent) = arena.nodes[child_handle].parent {
                    arena.remove_child_from(old_parent, child_handle);
                }
                arena.nodes[child_handle].parent = Some(*parent);
                arena.nodes[*parent].children.push(child_handle);

                self.set_node_depth_and_check(&mut arena, child_handle, *parent);
            }
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &Self::Handle,
        prev_element: &Self::Handle,
        child: NodeOrText<Self::Handle>,
    ) {
        let has_parent = self.arena.borrow().nodes[*prev_element].parent.is_some();
        if has_parent {
            self.append_before_sibling(prev_element, child);
        } else {
            self.append(element, child);
        }
    }

    fn append_doctype_to_document(
        &self,
        name: StrTendril,
        public_id: StrTendril,
        system_id: StrTendril,
    ) {
        let mut arena = self.arena.borrow_mut();
        let doctype = arena.alloc(
            NodeData::Doctype {
                _name: name.to_string(),
                _public_id: public_id.to_string(),
                _system_id: system_id.to_string(),
            },
            0,
        );
        arena.nodes[doctype].parent = Some(self.document_handle);
        arena.nodes[self.document_handle].children.push(doctype);
        let len = arena.nodes.len();
        drop(arena);
        self.check_node_limit(len);
    }

    fn get_template_contents(&self, target: &Self::Handle) -> Self::Handle {
        let arena = self.arena.borrow();
        match &arena.nodes[*target].data {
            NodeData::Element {
                template_contents: Some(contents),
                ..
            } => *contents,
            _ => panic!("get_template_contents called on non-template node {target}"),
        }
    }

    fn same_node(&self, x: &Self::Handle, y: &Self::Handle) -> bool {
        *x == *y
    }

    fn set_quirks_mode(&self, _mode: QuirksMode) {}

    fn append_before_sibling(&self, sibling: &Self::Handle, new_node: NodeOrText<Self::Handle>) {
        let mut arena = self.arena.borrow_mut();
        let parent = arena.nodes[*sibling]
            .parent
            .expect("append_before_sibling: sibling must have a parent");

        match new_node {
            NodeOrText::AppendText(tendril) => {
                let text_slice: &str = &tendril;
                if text_slice.is_empty() {
                    return;
                }
                let sibling_idx = arena.nodes[parent]
                    .children
                    .iter()
                    .rposition(|&h| h == *sibling)
                    .expect("sibling missing in parent children");

                if sibling_idx > 0 {
                    let prev_handle = arena.nodes[parent].children[sibling_idx - 1];
                    if let NodeData::Text { ref mut text } = arena.nodes[prev_handle].data {
                        text.push_str(text_slice);
                        return;
                    }
                }
                let parent_depth = arena.nodes[parent].depth;
                let child_handle = arena.alloc(
                    NodeData::Text {
                        text: text_slice.to_owned(),
                    },
                    parent_depth,
                );
                arena.nodes[child_handle].parent = Some(parent);
                arena.nodes[parent]
                    .children
                    .insert(sibling_idx, child_handle);
                let len = arena.nodes.len();
                drop(arena);
                self.check_node_limit(len);
            }
            NodeOrText::AppendNode(child_handle) => {
                if let Some(old_parent) = arena.nodes[child_handle].parent {
                    arena.remove_child_from(old_parent, child_handle);
                }
                arena.nodes[child_handle].parent = Some(parent);
                let sibling_idx = arena.nodes[parent]
                    .children
                    .iter()
                    .rposition(|&h| h == *sibling)
                    .expect("sibling missing in parent children");
                arena.nodes[parent]
                    .children
                    .insert(sibling_idx, child_handle);

                self.set_node_depth_and_check(&mut arena, child_handle, parent);
            }
        }
    }

    fn add_attrs_if_missing(&self, target: &Self::Handle, attrs: Vec<Attribute>) {
        let mut arena = self.arena.borrow_mut();
        if let NodeData::Element {
            attrs: ref mut existing,
            ..
        } = arena.nodes[*target].data
        {
            for attr in attrs {
                if !existing.iter().any(|e| e.name == attr.name) {
                    existing.push(attr);
                }
            }
        }
    }

    fn remove_from_parent(&self, target: &Self::Handle) {
        let mut arena = self.arena.borrow_mut();
        if let Some(parent) = arena.nodes[*target].parent.take() {
            arena.remove_child_from(parent, *target);
        }
    }

    fn reparent_children(&self, node: &Self::Handle, new_parent: &Self::Handle) {
        let mut arena = self.arena.borrow_mut();
        let children = std::mem::take(&mut arena.nodes[*node].children);
        let new_parent_depth = arena.nodes[*new_parent].depth;
        let mut max_child_deepest = arena.nodes[*new_parent].max_depth;

        for &child in &children {
            arena.nodes[child].parent = Some(*new_parent);
            let is_elem = matches!(arena.nodes[child].data, NodeData::Element { .. });
            let child_depth = if is_elem {
                new_parent_depth + 1
            } else {
                new_parent_depth
            };
            let subtree_height = arena.nodes[child]
                .max_depth
                .saturating_sub(arena.nodes[child].depth);
            arena.nodes[child].depth = child_depth;
            arena.nodes[child].max_depth = child_depth + subtree_height;

            let mut deepest = arena.nodes[child].max_depth;
            if let NodeData::Element {
                template_contents: Some(tc),
                ..
            } = arena.nodes[child].data
            {
                let tc_height = arena.nodes[tc]
                    .max_depth
                    .saturating_sub(arena.nodes[tc].depth);
                arena.nodes[tc].depth = child_depth;
                arena.nodes[tc].max_depth = child_depth + tc_height;
                deepest = deepest.max(arena.nodes[tc].max_depth);
            }

            if deepest > self.max_dom_depth && self.limit_hit.get().is_none() {
                self.limit_hit.set(Some(LimitExceeded::NestingTooDeep {
                    limit: self.max_dom_depth as u16,
                }));
            }
            if deepest > max_child_deepest {
                max_child_deepest = deepest;
            }
        }
        arena.nodes[*new_parent].children.extend(children);

        arena.nodes[*new_parent].max_depth =
            arena.nodes[*new_parent].max_depth.max(max_child_deepest);
    }

    fn is_mathml_annotation_xml_integration_point(&self, handle: &Self::Handle) -> bool {
        let arena = self.arena.borrow();
        match &arena.nodes[*handle].data {
            NodeData::Element {
                mathml_annotation_xml_integration_point,
                ..
            } => *mathml_annotation_xml_integration_point,
            _ => false,
        }
    }
}

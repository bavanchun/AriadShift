use ariad_core::{
    ir::{Block, Document},
    json_depth::JsonDepthError,
    limits::Limits,
};
use ariad_host::ir_io;

#[test]
fn reads_sixty_four_nested_ir_blocks() {
    let mut nested = Block::Paragraph {
        content: Vec::new(),
    };
    for _ in 0..64 {
        nested = Block::Quote {
            blocks: vec![nested],
        };
    }
    let document = Document {
        body: vec![nested],
        ..Document::default()
    };
    let json = serde_json::to_vec(&document).unwrap();

    let read = ir_io::read(json.as_slice(), &Limits::local()).unwrap();

    assert_eq!(read, document);
}

#[test]
fn rejects_byte_limit_exceeded() {
    let document = Document {
        body: vec![Block::Paragraph {
            content: Vec::new(),
        }],
        ..Document::default()
    };
    let json = serde_json::to_vec(&document).unwrap();

    let mut limits = Limits::local();
    limits.max_ir_json_bytes = 20;

    let result = ir_io::read(json.as_slice(), &limits);
    assert!(matches!(
        result,
        Err(ir_io::ReadError::ByteLimitExceeded { limit: 20 })
    ));
}

#[test]
fn rejects_depth_limit_exceeded() {
    let budget = ariad_core::json_depth::json_depth_budget_for_nesting(1);
    let mut deep_json = Vec::new();
    for _ in 0..budget + 2 {
        deep_json.extend_from_slice(b"{\"a\":");
    }
    deep_json.extend_from_slice(b"null");
    for _ in 0..budget + 2 {
        deep_json.extend_from_slice(b"}");
    }

    let mut limits = Limits::local();
    limits.max_nesting_depth = 1;

    let result = ir_io::read(deep_json.as_slice(), &limits);
    assert!(matches!(
        result,
        Err(ir_io::ReadError::DepthExceeded(
            JsonDepthError::DepthExceeded { .. }
        ))
    ));
}

#[test]
fn rejects_block_limit_exceeded() {
    let mut blocks = Vec::new();
    for _ in 0..10 {
        blocks.push(Block::Paragraph {
            content: Vec::new(),
        });
    }
    let document = Document {
        body: blocks,
        ..Document::default()
    };
    let json = serde_json::to_vec(&document).unwrap();

    let mut limits = Limits::local();
    limits.max_blocks = 5;

    let result = ir_io::read(json.as_slice(), &limits);
    assert!(matches!(
        result,
        Err(ir_io::ReadError::BlockLimitExceeded {
            count: 10,
            limit: 5
        })
    ));
}

#[test]
fn exact_byte_limit_accepted_and_one_byte_over_rejected() {
    let document = Document {
        body: vec![Block::Paragraph {
            content: Vec::new(),
        }],
        ..Document::default()
    };
    let json = serde_json::to_vec(&document).unwrap();
    let exact_len = json.len() as u64;

    let mut limits = Limits::local();
    limits.max_ir_json_bytes = exact_len;

    // Exactly exact_len bytes -> Accepted!
    let read = ir_io::read(json.as_slice(), &limits).expect("exact byte limit must be accepted");
    assert_eq!(read, document);

    // One byte more (e.g. trailing space) -> Rejected!
    let mut over_by_one = json.clone();
    over_by_one.push(b' ');
    let result = ir_io::read(over_by_one.as_slice(), &limits);
    assert!(matches!(
        result,
        Err(ir_io::ReadError::ByteLimitExceeded { limit }) if limit == exact_len
    ));

    // When the limit is one byte less than the document length -> Rejected!
    let mut limits_under = Limits::local();
    limits_under.max_ir_json_bytes = exact_len - 1;
    let result_under = ir_io::read(json.as_slice(), &limits_under);
    assert!(matches!(
        result_under,
        Err(ir_io::ReadError::ByteLimitExceeded { limit }) if limit == exact_len - 1
    ));
}

#[test]
fn reads_sixty_four_deep_nested_list() {
    use ariad_core::ir::ListItem;

    let mut inner = Block::Paragraph {
        content: Vec::new(),
    };
    for _ in 0..64 {
        inner = Block::List {
            ordered: false,
            start: None,
            tight: true,
            items: vec![ListItem {
                checked: None,
                blocks: vec![inner],
            }],
        };
    }
    let document = Document {
        body: vec![inner],
        ..Document::default()
    };
    let json = serde_json::to_vec(&document).unwrap();

    let read = ir_io::read(json.as_slice(), &Limits::local()).unwrap();
    assert_eq!(read, document);
}

#[test]
fn reads_sixty_four_deep_nested_table_in_cell() {
    use ariad_core::ir::{Alignment, ColumnSpec, TableCell};

    let mut inner = Block::Paragraph {
        content: Vec::new(),
    };
    for _ in 0..64 {
        inner = Block::Table {
            caption: None,
            columns: vec![ColumnSpec {
                align: Alignment::Default,
            }],
            head: Vec::new(),
            body: vec![vec![TableCell {
                rowspan: 1,
                colspan: 1,
                header: None,
                blocks: vec![inner],
            }]],
            footnotes: Vec::new(),
        };
    }
    let document = Document {
        body: vec![inner],
        ..Document::default()
    };
    let json = serde_json::to_vec(&document).unwrap();

    let read = ir_io::read(json.as_slice(), &Limits::local()).unwrap();
    assert_eq!(read, document);
}

#[test]
fn rejects_over_deep_json_before_tree_grows() {
    let budget = ariad_core::json_depth::json_depth_budget_for_nesting(10);
    let mut payload = Vec::new();
    for _ in 0..1000 {
        payload.extend_from_slice(b"{\"k\":");
    }
    payload.extend_from_slice(b"1");
    for _ in 0..1000 {
        payload.extend_from_slice(b"}");
    }

    let mut limits = Limits::local();
    limits.max_nesting_depth = 10;

    let result = ir_io::read(payload.as_slice(), &limits);
    assert!(matches!(
        result,
        Err(ir_io::ReadError::DepthExceeded(
            JsonDepthError::DepthExceeded { depth, max_depth }
        )) if depth == budget + 1 && max_depth == budget
    ));
}

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

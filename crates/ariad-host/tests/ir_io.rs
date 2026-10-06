use ariad_core::ir::{Block, Document};
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

    let read = ir_io::read(json.as_slice()).unwrap();

    assert_eq!(read, document);
}

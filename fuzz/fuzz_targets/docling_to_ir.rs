#![no_main]

use ariad_core::{
    docling::{ReadError, read},
    ir::{AssetRef, Block},
    limits::Limits,
};
use ariad_fuzz::{assert_blocks_depth, assert_document_nfc};
use libfuzzer_sys::fuzz_target;
use serde::Deserialize;

const MAX_INPUT_BYTES: usize = 256 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }

    // Production local limits, then small limits that make every budget reachable.
    let small = Limits {
        max_ir_json_bytes: 16 * 1024,
        max_nesting_depth: 8,
        max_blocks: 32,
        max_asset_bytes: Some(512),
        ..Limits::local()
    };
    for limits in [Limits::local(), small] {
        match read(data, &limits) {
            Ok(output) => {
                let document = output.document;
                assert!(data.len() as u64 <= limits.max_ir_json_bytes);
                assert!(
                    document.block_count() <= limits.max_blocks as usize,
                    "block count {} exceeds max_blocks {}",
                    document.block_count(),
                    limits.max_blocks
                );
                let max_depth = limits.max_nesting_depth as usize;
                assert_blocks_depth(&document.body, max_depth);
                assert_blocks_depth(&document.furniture, max_depth);
                assert_document_nfc(&document, max_depth);

                // Every figure points at an embedded asset within the asset limit.
                for block in document.body.iter().chain(&document.furniture) {
                    if let Block::Figure {
                        asset: AssetRef::Asset { id },
                        ..
                    } = block
                    {
                        let asset = document.assets.get(id).expect("figure asset is embedded");
                        if let Some(limit) = limits.max_asset_bytes {
                            assert!(asset.bytes.len() as u64 <= limit);
                        }
                    }
                }

                // Layout positions name blocks that exist.
                if let Some(layout) = &document.layout {
                    for path in layout.blocks.keys() {
                        let mut parts = path.split('/');
                        let root = parts.next().unwrap();
                        let index: usize = parts.next().unwrap().parse().unwrap();
                        let len = if root == "body" {
                            document.body.len()
                        } else {
                            assert_eq!(root, "furniture");
                            document.furniture.len()
                        };
                        assert!(index < len, "layout path {path} names a missing block");
                    }
                }

                let serialized =
                    serde_json::to_string(&document).expect("mapped IR serializes to JSON");
                let mut deserializer = serde_json::Deserializer::from_str(&serialized);
                deserializer.disable_recursion_limit();
                let round_trip = ariad_core::ir::Document::deserialize(&mut deserializer)
                    .expect("serialized IR deserializes");
                assert_eq!(document, round_trip);
            }
            Err(ReadError::InputTooLarge { limit }) => {
                assert!(data.len() as u64 > limit);
            }
            Err(
                ReadError::InvalidLimits(_)
                | ReadError::JsonTooDeep(_)
                | ReadError::InvalidJson { .. }
                | ReadError::NotDoclingDocument
                | ReadError::UnsupportedVersion
                | ReadError::NestingTooDeep { .. }
                | ReadError::TooManyBlocks { .. }
                | ReadError::ContentTooLarge { .. },
            ) => {}
        }
    }
});

//! Exercise the conversion boundary with the Windows executable's default stack size.

use ariad_core::{limits::Limits, pandoc::MapOutput, pipeline, reader::markdown};

#[test]
fn deep_list_conversion_fits_a_one_mib_stack() {
    std::thread::Builder::new()
        .name("deep-list-one-mib".into())
        .stack_size(1024 * 1024)
        .spawn(|| {
            let limits = Limits::local();
            let read = markdown::read(
                include_str!("../../../fixtures/md/en-deep-list.md"),
                &limits,
            )
            .unwrap();
            assert_eq!(read.document.block_count(), 128);
            let bytes = serde_json::to_vec(&read.document).unwrap();
            let restored = pipeline::read_ir_json(&bytes, &limits).unwrap();
            assert_eq!(serde_json::to_vec(&restored).unwrap(), bytes);
            let mapped = MapOutput::for_docx(&restored);
            assert!(!mapped.pandoc.blocks.is_empty());
            assert!(!serde_json::to_vec(&mapped.pandoc).unwrap().is_empty());
        })
        .unwrap()
        .join()
        .unwrap();
}

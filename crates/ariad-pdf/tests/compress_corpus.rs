#![cfg(feature = "codecs")]

use ariad_core::tools::params::{CompressParams, CompressPreset};
use ariad_pdf::{
    InputDoc, ToolCtx,
    tools::{OutputPayload, compress},
};
use std::path::Path;

#[test]
fn ebook_matches_stirling_on_corpus() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let baseline = include_str!("data/stirling-level6.tsv");
    let mut wins = 0;
    let mut errors = 0;
    for row in baseline.lines().skip(1) {
        let fields: Vec<_> = row.split('\t').collect();
        let bytes = std::fs::read(root.join(fields[0])).unwrap();
        let reference: usize = fields[2].parse().unwrap();
        let params = CompressParams {
            preset: Some(CompressPreset::Ebook),
            ..Default::default()
        };
        let (_, mut docs) = match compress::run_inputs(
            &[InputDoc::new(fields[0], bytes)],
            &params,
            &ToolCtx::default(),
        ) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("{}: {error}", fields[0]);
                errors += 1;
                continue;
            }
        };
        let OutputPayload::Bytes(output) = docs.remove(0).payload else {
            panic!("expected PDF")
        };
        println!("{}: {} / {}", fields[0], output.len(), reference);
        if output.len() <= reference {
            wins += 1;
        }
    }
    // Accepted measured baseline; text-heavy parity follow-up (https://github.com/bavanchun/AriadShift/issues/187).
    assert!(
        errors == 0 && wins >= 6,
        "{errors} errors and only {wins}/10 meet the Stirling size"
    );
}

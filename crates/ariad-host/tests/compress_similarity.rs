use ariad_host::tools::{InputRef, OutputTarget, ToolRequest, run_tool};
use serde_json::json;
use std::{path::Path, time::Instant};

#[test]
fn corpus_size_and_similarity_against_stirling() {
    if std::env::var_os("ASHIFT_PACK_PDFIUM").is_none() {
        eprintln!("skipped: ASHIFT_PACK_PDFIUM is not set");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let reference = include_str!("../../ariad-pdf/tests/data/stirling-level6.tsv");
    let temporary = tempfile::tempdir().unwrap();
    let spec = ariad_core::tools::specs::optimize::specs()
        .into_iter()
        .find(|s| s.id == "pdf.compress")
        .unwrap();
    let mut wins = 0;
    let mut target_hits = 0;
    let mut errors = 0;
    for (index, row) in reference.lines().skip(1).enumerate() {
        let fields: Vec<_> = row.split('\t').collect();
        let input = root.join(fields[0]);
        let input_bytes: usize = fields[1].parse().unwrap();
        let stirling: usize = fields[2].parse().unwrap();
        let destination = temporary.path().join(format!("{index}.pdf"));
        let request = ToolRequest::new(
            "pdf.compress",
            vec![InputRef::new(&input)],
            OutputTarget::Path(destination.clone()),
        )
        .with_spec(spec);
        let start = Instant::now();
        let result = match run_tool(&request) {
            Ok(result) => result,
            Err(error) => {
                eprintln!("{}: {error}", fields[0]);
                errors += 1;
                continue;
            }
        };
        let output = std::fs::metadata(&destination).unwrap().len() as usize;
        let score = result
            .metrics
            .get("similarity")
            .and_then(|v| v.as_f64())
            .expect("similarity measured");
        println!(
            "{}\t{}\t{}\t{}\t{:.4}\t{:.4}\t{}",
            fields[0],
            input_bytes,
            stirling,
            output,
            output as f64 / input_bytes as f64,
            score,
            start.elapsed().as_millis()
        );
        if output <= stirling && score >= ariad_host::tools::similarity::SIMILARITY_FLOOR {
            wins += 1;
        }
        if let Some(dir) = std::env::var_os("ARIAD_TEST_STIRLING_DIR") {
            let reference_output = Path::new(&dir).join(input.file_name().unwrap());
            match ariad_host::tools::similarity::measure(
                &input,
                &reference_output,
                &Default::default(),
                None,
                &Default::default(),
            ) {
                Ok(score) => println!("stirling {}: {:.4}", fields[0], score),
                Err(_) => eprintln!("stirling {}: not measured", fields[0]),
            }
        }

        let target_path = temporary.path().join(format!("target-{index}.pdf"));
        let target = (input_bytes / 2).max(1024);
        let target_request = ToolRequest::new(
            "pdf.compress",
            vec![InputRef::new(&input)],
            OutputTarget::Path(target_path.clone()),
        )
        .with_spec(spec)
        .with_params(json!({"target_bytes":target}));
        let target_result = run_tool(&target_request).unwrap();
        let target_size = std::fs::metadata(target_path).unwrap().len() as usize;
        let target_similarity = target_result
            .metrics
            .get("similarity")
            .and_then(|v| v.as_f64())
            .expect("target similarity measured");
        if target_similarity < ariad_host::tools::similarity::SIMILARITY_FLOOR {
            assert!(
                target_result
                    .warnings
                    .iter()
                    .any(|warning| warning.code == "similarity_below_floor")
            );
        }
        let within_five_percent = target_size.abs_diff(target) as f64 <= target as f64 * 0.05;
        if within_five_percent {
            target_hits += 1;
        }
        if target_size > target {
            assert!(
                target_result
                    .warnings
                    .iter()
                    .any(|w| w.code == "target_not_reached")
            );
        }
        println!(
            "target {}: {} bytes, similarity {:.4}, within 5%: {}",
            fields[0], target_size, target_similarity, within_five_percent
        );
    }
    println!("acceptance: {wins}/10; target hit rate: {target_hits}/10; errors: {errors}");
    assert!(
        errors == 0 && wins >= 7,
        "{errors} errors; only {wins}/10 meet size and similarity"
    );
}

#[test]
fn encrypted_inputs_return_exit_twelve() {
    let Some(qpdf_dir) = std::env::var_os("ARIAD_TEST_QPDF_DIR") else {
        eprintln!("skipped: ARIAD_TEST_QPDF_DIR is not set");
        return;
    };
    let qpdf_dir = Path::new(&qpdf_dir);
    let plaintext = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/encrypted-owner-only.plaintext.pdf");
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("encrypted.pdf");
    let status = std::process::Command::new(qpdf_dir.join("bin/qpdf"))
        .env("LD_LIBRARY_PATH", qpdf_dir.join("lib"))
        .args([
            "--encrypt",
            "--user-password=user",
            "--owner-password=owner",
            "--bits=256",
            "--",
        ])
        .arg(plaintext)
        .arg(&input)
        .status()
        .unwrap();
    assert!(status.success());
    for tool in ["pdf.compress", "pdf.grayscale"] {
        let spec = ariad_core::tools::specs::optimize::specs()
            .into_iter()
            .find(|spec| spec.id == tool)
            .unwrap();
        let request = ToolRequest::new(
            tool,
            vec![InputRef::new(&input)],
            OutputTarget::Path(directory.path().join(format!("{tool}.pdf"))),
        )
        .with_spec(spec);
        assert_eq!(run_tool(&request).unwrap_err().exit_code(), 12);
    }
}

#[test]
fn image_resource_limits_map_to_exit_four() {
    let error = ariad_pdf::PdfError::LimitExceeded(
        ariad_core::reader::pdf::ReadError::DecompressedTooLarge { limit: 1024 },
    );
    let mapped = ariad_host::tools::inprocess::map_pdf_error(
        error,
        &ariad_host::tools::SecretScrubber::default(),
    );
    assert_eq!(mapped.exit_code(), 4);
}

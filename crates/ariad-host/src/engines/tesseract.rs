//! Tesseract adapter for the ariad-engine/2 internal OCR operations.
use super::{
    adapter,
    common::{self, EngineFailure},
    tesseract_hocr, tesseract_image,
};
use ariad_core::{
    ocr::{MAX_OCR_JSON_BYTES, OcrOrientation},
    protocol::{
        ErrorCode, ToolAvailability, ToolStatus,
        v2::{EventV2, PROTOCOL_V2, RequestV2, ToolCapability},
    },
    tools::params::{OcrOrientationParams, OcrRecognizeParams},
};
use image::GrayImage;
use std::{
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    thread,
    time::{Duration, Instant},
};

const TOOLS: &[&str] = &["ocr.recognize", "ocr.orientation"];
const MAX_ERROR_BYTES: u64 = 1024 * 1024;
const INSUFFICIENT_ORIENTATION_TEXT: &str = "Not enough text to detect orientation.";

fn pack() -> Result<(PathBuf, PathBuf), EngineFailure> {
    let missing = || EngineFailure::tool_missing("The verified Tesseract pack is unavailable.");
    if cfg!(any(target_os = "windows", target_env = "musl")) {
        return Err(missing());
    }
    let (program, root) = adapter::program(
        "tesseract",
        "ASHIFT_TESSERACT_PROGRAM",
        "The verified Tesseract pack is unavailable.",
    )?;
    let declared = PathBuf::from(std::env::var_os("TESSDATA_PREFIX").ok_or_else(missing)?);
    let expected = root
        .join("tessdata")
        .canonicalize()
        .map_err(|_| missing())?;
    if !expected.starts_with(&root)
        || !expected.is_dir()
        || declared.canonicalize().map_err(|_| missing())? != expected
    {
        return Err(missing());
    }
    Ok((program, expected))
}

fn model_path(directory: &Path, language: &str) -> Result<(), EngineFailure> {
    let missing = || EngineFailure::tool_missing("A requested OCR language is not installed.");
    let path = directory
        .join(format!("{language}.traineddata"))
        .canonicalize()
        .map_err(|_| missing())?;
    if !path.is_file() || !path.starts_with(directory) {
        return Err(missing());
    }
    Ok(())
}

fn pgm(image: &GrayImage, directory: &Path) -> Result<tempfile::NamedTempFile, EngineFailure> {
    // Tesseract's OSD treats the last dot as an extension delimiter. A leading-dot
    // tempfile makes it try to open the containing directory as a zone file.
    let mut output = tempfile::Builder::new()
        .prefix("ocr-")
        .suffix(".pgm")
        .tempfile_in(directory)
        .map_err(|_| EngineFailure::io())?;
    write!(output, "P5\n{} {}\n255\n", image.width(), image.height())
        .map_err(|_| EngineFailure::io())?;
    output
        .write_all(image.as_raw())
        .map_err(|_| EngineFailure::io())?;
    output.flush().map_err(|_| EngineFailure::io())?;
    Ok(output)
}

fn invoke(program: &Path, args: &[String], deadline: Instant) -> Result<Vec<u8>, EngineFailure> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Command::env_clear(&mut command);
    Command::env(&mut command, "OMP_THREAD_LIMIT", "1");
    let mut child = command
        .spawn()
        .map_err(|_| EngineFailure::tool_missing("Could not start Tesseract."))?;
    let stdout = child.stdout.take().ok_or_else(EngineFailure::io)?;
    let stderr = child.stderr.take().ok_or_else(EngineFailure::io)?;
    fn capture(mut reader: impl Read, cap: usize) -> Result<(Vec<u8>, bool), std::io::Error> {
        let mut result = Vec::new();
        let mut block = [0u8; 8192];
        let mut truncated = false;
        loop {
            let n = reader.read(&mut block)?;
            if n == 0 {
                break;
            }
            let keep = cap.saturating_sub(result.len()).min(n);
            result.extend_from_slice(&block[..keep]);
            truncated |= keep < n;
        }
        Ok((result, truncated))
    }
    let out_thread = thread::spawn(move || capture(stdout, MAX_OCR_JSON_BYTES));
    let err_thread = thread::spawn(move || capture(stderr, MAX_ERROR_BYTES as usize));
    let status = loop {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = out_thread.join();
            let _ = err_thread.join();
            return Err(EngineFailure::limit_exceeded());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(EngineFailure::io());
            }
        }
    };
    let (output, output_cut) = out_thread
        .join()
        .map_err(|_| EngineFailure::io())?
        .map_err(|_| EngineFailure::io())?;
    let (errors, error_cut) = err_thread
        .join()
        .map_err(|_| EngineFailure::io())?
        .map_err(|_| EngineFailure::io())?;
    if output_cut || error_cut {
        return Err(EngineFailure::limit_exceeded());
    }
    if !status.success() {
        if args.windows(2).any(|pair| pair == ["--psm", "0"])
            && errors
                .windows(b"Too few characters. Skipping this page".len())
                .any(|part| part == b"Too few characters. Skipping this page")
        {
            return Err(EngineFailure::new(
                ErrorCode::EngineFailure,
                INSUFFICIENT_ORIENTATION_TEXT,
            ));
        }
        return Err(EngineFailure::new(
            ErrorCode::EngineFailure,
            "Tesseract could not recognize the image.",
        ));
    }
    Ok(output)
}

fn detect_orientation(
    program: &Path,
    argv: &[String],
    deadline: Instant,
) -> Result<OcrOrientation, EngineFailure> {
    match invoke(program, argv, deadline) {
        Ok(bytes) => orientation(&bytes),
        Err(error) if error.message == INSUFFICIENT_ORIENTATION_TEXT => Ok(OcrOrientation {
            rotation_cw: 0,
            confidence: 0.0,
            script: None,
            script_confidence: None,
        }),
        Err(error) => Err(error),
    }
}

fn orientation(bytes: &[u8]) -> Result<OcrOrientation, EngineFailure> {
    let text = std::str::from_utf8(bytes).map_err(|_| EngineFailure::invalid_request())?;
    let field = |prefix: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(prefix).map(str::trim))
    };
    let rotation_cw = field("Rotate:")
        .ok_or_else(EngineFailure::invalid_request)?
        .parse::<u16>()
        .map_err(|_| EngineFailure::invalid_request())?;
    let confidence = field("Orientation confidence:")
        .ok_or_else(EngineFailure::invalid_request)?
        .parse::<f32>()
        .map_err(|_| EngineFailure::invalid_request())?;
    if !matches!(rotation_cw, 0 | 90 | 180 | 270) || !confidence.is_finite() || confidence < 0.0 {
        return Err(EngineFailure::invalid_request());
    }
    let script = field("Script:").map(str::to_owned).filter(|value| {
        !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b' ' || c == b'-')
    });
    let script_confidence = field("Script confidence:")
        .and_then(|s| s.parse::<f32>().ok())
        .filter(|n| n.is_finite() && *n >= 0.0);
    Ok(OcrOrientation {
        rotation_cw,
        confidence,
        script,
        script_confidence,
    })
}

fn args(path: &Path, data: &Path, dpi: u32) -> Result<Vec<String>, EngineFailure> {
    Ok(vec![
        path.to_str()
            .ok_or_else(EngineFailure::invalid_request)?
            .to_owned(),
        "stdout".into(),
        "--dpi".into(),
        dpi.to_string(),
        "--tessdata-dir".into(),
        data.to_str()
            .ok_or_else(EngineFailure::invalid_request)?
            .to_owned(),
    ])
}

/// Serve internal OCR requests in a sandboxed engine process.
pub fn serve(input: impl BufRead, output: impl Write) -> ExitCode {
    std::panic::set_hook(Box::new(|_| {
        eprintln!("Tesseract adapter failed unexpectedly")
    }));
    common::serve_v2(input, output, |request, out| match request {
        RequestV2::Describe { protocol, .. } => {
            let (version, status) = match pack().and_then(|(program, _)| {
                invoke(
                    &program,
                    &["--version".into()],
                    Instant::now() + Duration::from_secs(3),
                )
            }) {
                Ok(output) => {
                    let text = String::from_utf8_lossy(&output);
                    let version = text
                        .lines()
                        .next()
                        .and_then(|line| line.strip_prefix("tesseract "))
                        .map(str::to_owned);
                    let status = if version.as_deref() == Some("5.5.4") {
                        ToolAvailability::Found
                    } else {
                        ToolAvailability::WrongVersion
                    };
                    (version, status)
                }
                Err(_) => (None, ToolAvailability::Missing),
            };
            common::emit_capabilities_v2(
                out,
                &protocol,
                EventV2::Capabilities {
                    protocol: PROTOCOL_V2.into(),
                    engine: "tesseract".into(),
                    version: env!("CARGO_PKG_VERSION").into(),
                    tool: ToolStatus {
                        name: "tesseract".into(),
                        version,
                        status,
                    },
                    license: "Apache-2.0".into(),
                    routes: vec![],
                    tools: TOOLS
                        .iter()
                        .map(|id| ToolCapability {
                            id: (*id).into(),
                            version: "1".into(),
                        })
                        .collect(),
                    enforces_memory_limit: false,
                    supports_session: false,
                    models: None,
                },
            )
        }
        RequestV2::Convert { .. } => Err(EngineFailure::unsupported_route()),
        RequestV2::RunTool {
            protocol,
            tool,
            inputs,
            output,
            params,
            params_file,
            work_dir,
            limits,
            ..
        } => {
            if !TOOLS.contains(&tool.as_str()) || inputs.is_empty() || inputs.len() > 200 {
                return Err(EngineFailure::unsupported_route());
            }
            if params_file.is_some() && params.as_object().is_some_and(|m| !m.is_empty()) {
                return Err(EngineFailure::invalid_request());
            }
            let (paths, params) = common::validate_run_tool(
                &protocol,
                &inputs,
                &output,
                &params,
                params_file.as_deref(),
                &work_dir,
                &limits,
            )?;
            if limits
                .max_pages
                .is_some_and(|max| paths.inputs.len() > max as usize)
            {
                return Err(EngineFailure::limit_exceeded());
            }
            let (program, pack_data) = pack()?;
            let (dpi, languages, auto_orient) = if tool == "ocr.recognize" {
                let p: OcrRecognizeParams =
                    serde_json::from_value(params).map_err(|_| EngineFailure::invalid_request())?;
                (p.dpi, p.languages, p.auto_orient)
            } else {
                let p: OcrOrientationParams =
                    serde_json::from_value(params).map_err(|_| EngineFailure::invalid_request())?;
                (p.dpi, Vec::new(), false)
            };
            if !(1..=1200).contains(&dpi) {
                return Err(EngineFailure::invalid_request());
            }
            let need_extra = languages
                .iter()
                .any(|language| model_path(&pack_data, language).is_err());
            let combined = if need_extra {
                let directory = tempfile::Builder::new()
                    .prefix("ocr-data-")
                    .tempdir_in(&paths.work_dir)
                    .map_err(|_| EngineFailure::io())?;
                for language in languages
                    .iter()
                    .map(String::as_str)
                    .chain((tool == "ocr.orientation" || auto_orient).then_some("osd"))
                {
                    let name = format!("{language}.traineddata");
                    let source = if model_path(&pack_data, language).is_ok() {
                        pack_data.join(&name)
                    } else {
                        let staged = paths.work_dir.join("ocr-models").join(&name);
                        if !crate::packs::languages::verify_model(&staged, language)
                            .unwrap_or(false)
                        {
                            return Err(EngineFailure::tool_missing(
                                "A requested OCR language is not installed or failed verification.",
                            ));
                        }
                        staged
                    };
                    let destination = directory.path().join(name);
                    std::fs::copy(source, &destination).map_err(|_| EngineFailure::io())?;
                    if !pack_data.join(format!("{language}.traineddata")).is_file()
                        && !crate::packs::languages::verify_model(&destination, language)
                            .unwrap_or(false)
                    {
                        return Err(EngineFailure::tool_missing(
                            "An OCR language failed verification.",
                        ));
                    }
                }
                Some(directory)
            } else {
                None
            };
            let combined_path = combined
                .as_ref()
                .map(|dir| dir.path().canonicalize())
                .transpose()
                .map_err(|_| EngineFailure::io())?;
            let data = combined_path.as_deref().unwrap_or(&pack_data);
            for language in &languages {
                model_path(data, language)?;
            }
            if tool == "ocr.orientation" || auto_orient {
                model_path(data, "osd")?;
            }
            let deadline = Instant::now()
                .checked_add(Duration::from_secs(limits.timeout_s.unwrap_or(600)))
                .ok_or_else(EngineFailure::limit_exceeded)?;
            let mut artifacts = Vec::new();
            let mut output_bytes = 0_u64;
            for (index, input) in paths.inputs.iter().enumerate() {
                if Instant::now() >= deadline {
                    return Err(EngineFailure::limit_exceeded());
                }
                let mut image = tesseract_image::decode(input, &limits, deadline)?;
                let original = pgm(&image, &paths.work_dir)?;
                let (extension, json) = if tool == "ocr.orientation" {
                    let mut argv = args(original.path(), data, dpi)?;
                    argv.extend(["--psm".into(), "0".into()]);
                    (
                        "osd",
                        serde_json::to_vec(&detect_orientation(&program, &argv, deadline)?)
                            .map_err(|_| EngineFailure::io())?,
                    )
                } else {
                    let rotation = if auto_orient {
                        let mut argv = args(original.path(), data, dpi)?;
                        argv.extend(["--psm".into(), "0".into()]);
                        detect_orientation(&program, &argv, deadline)?.rotation_cw
                    } else {
                        0
                    };
                    image = match rotation {
                        90 => image::imageops::rotate90(&image),
                        180 => image::imageops::rotate180(&image),
                        270 => image::imageops::rotate270(&image),
                        _ => image,
                    };
                    let upright = pgm(&image, &paths.work_dir)?;
                    let mut argv = args(upright.path(), data, dpi)?;
                    argv.extend([
                        "-l".into(),
                        languages.join("+"),
                        "-c".into(),
                        "tessedit_create_hocr=1".into(),
                    ]);
                    let mut page = tesseract_hocr::parse(
                        &invoke(&program, &argv, deadline)?,
                        image.width(),
                        image.height(),
                        dpi,
                        languages.clone(),
                    )
                    .map_err(|_| {
                        EngineFailure::new(
                            ErrorCode::EngineFailure,
                            "Tesseract returned invalid hOCR.",
                        )
                    })?;
                    page.rotation_cw = rotation;
                    page.validate()
                        .map_err(|_| EngineFailure::invalid_request())?;
                    (
                        "ocr",
                        serde_json::to_vec(&page).map_err(|_| EngineFailure::io())?,
                    )
                };
                if json.len() > MAX_OCR_JSON_BYTES {
                    return Err(EngineFailure::limit_exceeded());
                }
                output_bytes = output_bytes
                    .checked_add(json.len() as u64)
                    .ok_or_else(EngineFailure::limit_exceeded)?;
                if output_bytes > limits.max_decompressed_bytes {
                    return Err(EngineFailure::limit_exceeded());
                }
                let path = paths
                    .output_dir
                    .join(format!("{}.{}.json", index + 1, extension));
                std::fs::write(&path, json).map_err(|_| EngineFailure::io())?;
                artifacts.push(path);
            }
            let refs: Vec<_> = artifacts.iter().map(|p| (p.as_path(), "json")).collect();
            common::emit_artifacts_success(out, &refs, None)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_osd_and_rejects_invalid_rotation() {
        let result = orientation(b"Page number: 0\nRotate: 90\nOrientation confidence: 13.52\nScript: Latin\nScript confidence: 3.91\n").unwrap();
        assert_eq!(result.rotation_cw, 90);
        assert_eq!(result.script.as_deref(), Some("Latin"));
        assert!(orientation(b"Rotate: 45\nOrientation confidence: 10\n").is_err());
        assert!(orientation(b"Rotate: 0\nOrientation confidence: NaN\n").is_err());
    }

    #[test]
    fn pgm_round_trip_header_and_pixels() {
        let dir = tempfile::tempdir().unwrap();
        let image = GrayImage::from_raw(2, 1, vec![1, 254]).unwrap();
        let file = pgm(&image, dir.path()).unwrap();
        assert_eq!(
            std::fs::read(file.path()).unwrap(),
            b"P5\n2 1\n255\n\x01\xfe"
        );
    }

    #[test]
    fn missing_program_and_expired_deadline_are_typed() {
        let absent = invoke(
            Path::new("/nonexistent/ariad-tesseract"),
            &[],
            Instant::now(),
        )
        .unwrap_err();
        assert_eq!(absent.code, ErrorCode::ToolMissing);
        let Some(root) = std::env::var_os("ARIAD_TEST_TESSERACT_DIR") else {
            eprintln!("skipping deadline check: development Tesseract is absent");
            return;
        };
        let program = PathBuf::from(root).join("bin/tesseract");
        let expired = invoke(&program, &["--version".into()], Instant::now()).unwrap_err();
        assert_eq!(expired.code, ErrorCode::LimitExceeded);
    }

    #[test]
    fn real_tesseract_recognizes_vietnamese_and_orientation() {
        let Some(root) = std::env::var_os("ARIAD_TEST_TESSERACT_DIR") else {
            eprintln!("skipping: development Tesseract is absent");
            return;
        };
        let root = PathBuf::from(root);
        let data = root.join("tessdata");
        let program = root.join("bin/tesseract");
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/scan/vi-garden-300dpi.png");
        let image = tesseract_image::decode(
            &fixture,
            &ariad_core::limits::Limits::local(),
            Instant::now() + Duration::from_secs(30),
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let file = pgm(&image, dir.path()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut argv = args(file.path(), &data, 300).unwrap();
        argv.extend([
            "-l".into(),
            "vie+eng".into(),
            "-c".into(),
            "tessedit_create_hocr=1".into(),
        ]);
        let html = invoke(&program, &argv, deadline).unwrap();
        let page = tesseract_hocr::parse(
            &html,
            image.width(),
            image.height(),
            300,
            vec!["vie".into(), "eng".into()],
        )
        .unwrap();
        let text = page
            .paragraphs
            .iter()
            .flat_map(|p| &p.lines)
            .flat_map(|l| &l.words)
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(text.contains("Khu vườn đọc sách"), "{text}");
        let mut argv = args(file.path(), &data, 300).unwrap();
        argv.extend(["--psm".into(), "0".into()]);
        let result_bytes = invoke(&program, &argv, deadline).unwrap();
        let result = orientation(&result_bytes).unwrap();
        assert_eq!(result.rotation_cw, 0);
    }
}

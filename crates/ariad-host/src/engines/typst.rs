//! `ashift __engine typst`: writes IR JSON as PDF through the embedded Typst compiler.
//!
//! Typesetting runs here, in a child process, so a document that makes the compiler misbehave
//! can never take the host down.

use std::{
    collections::BTreeMap,
    io::{BufRead, Write},
    process::ExitCode,
};

use ariad_core::{
    format::Format,
    protocol::{ErrorCode, Event, Request, RouteCapability, ToolAvailability, ToolStatus},
};
use serde_json::Value;

use super::common::{self, EngineFailure};

/// The version of the `typst` crate compiled into ashift.
pub const TYPST_VERSION: &str = "0.15.1";

const UNSUPPORTED_ROUTE: EngineFailure = EngineFailure::new(
    ErrorCode::UnsupportedRoute,
    "The Typst engine converts IR JSON to PDF only.",
);

/// Reads one engine request, emits NDJSON events, and returns the process exit status.
pub fn serve<R: BufRead, W: Write>(input: R, output: W) -> ExitCode {
    common::serve(input, output, execute)
}

fn capabilities() -> Event {
    Event::Capabilities {
        engine: super::TYPST.id.to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        tool: ToolStatus {
            name: "typst".to_owned(),
            version: Some(TYPST_VERSION.to_owned()),
            status: if cfg!(feature = "typst") {
                ToolAvailability::Found
            } else {
                ToolAvailability::Missing
            },
        },
        license: super::TYPST.license.to_owned(),
        routes: super::TYPST
            .routes
            .iter()
            .map(|(input, output)| RouteCapability {
                input: input.id().to_owned(),
                output: output.id().to_owned(),
            })
            .collect(),
        // Reading the IR is bounded by the request limits, but typesetting cannot cap its own
        // resident memory.
        supports_session: false,
        enforces_memory_limit: false,
        models: None,
    }
}

/// Request options the Typst engine reads.
#[derive(Clone, Debug, PartialEq)]
struct Settings {
    title_fallback: Option<String>,
    /// Unix seconds used for the document date and the compiler clock.
    timestamp: i64,
    date: (i32, u8, u8),
}

impl Settings {
    /// Reads `title_fallback` (string) and `timestamp` (integer Unix seconds); the timestamp
    /// defaults to the current time.
    fn from_options(options: &BTreeMap<String, Value>) -> Result<Self, EngineFailure> {
        let title_fallback = match options.get("title_fallback") {
            None | Some(Value::Null) => None,
            Some(Value::String(title)) => Some(title.clone()),
            Some(_) => return Err(EngineFailure::invalid_request()),
        };
        let timestamp = match options.get("timestamp") {
            None | Some(Value::Null) => jiff::Timestamp::now().as_second(),
            Some(value) => value.as_i64().ok_or_else(EngineFailure::invalid_request)?,
        };
        let date = calendar_date(timestamp).ok_or_else(EngineFailure::invalid_request)?;
        Ok(Self {
            title_fallback,
            timestamp,
            date,
        })
    }
}

/// The UTC calendar date of a Unix timestamp within the years 1 to 9999.
fn calendar_date(timestamp: i64) -> Option<(i32, u8, u8)> {
    let date = jiff::Timestamp::from_second(timestamp)
        .ok()?
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date();
    if !(1..=9999).contains(&date.year()) {
        return None;
    }
    Some((
        i32::from(date.year()),
        u8::try_from(date.month()).ok()?,
        u8::try_from(date.day()).ok()?,
    ))
}

fn execute<W: Write>(request: Request, output: &mut W) -> Result<(), EngineFailure> {
    match request {
        Request::Describe { protocol, .. } => {
            common::emit_capabilities(output, &protocol, capabilities())
        }
        Request::Convert {
            protocol,
            input,
            output: request_output,
            work_dir,
            options,
            limits,
            ..
        } => {
            let paths = common::validate_convert_paths(
                &protocol,
                &input,
                &request_output,
                &work_dir,
                &limits,
            )?;
            if input.format != Format::AriadIrJson.id() || request_output.format != Format::Pdf.id()
            {
                return Err(UNSUPPORTED_ROUTE);
            }
            let settings = Settings::from_options(&options)?;
            convert(&paths, &limits, &settings, output)
        }
    }
}

#[cfg(feature = "typst")]
fn convert<W: Write>(
    paths: &common::ConvertPaths,
    limits: &ariad_core::limits::Limits,
    settings: &Settings,
    output: &mut W,
) -> Result<(), EngineFailure> {
    use std::{fs, io::BufReader};

    use ariad_core::writer::typst::TypstOptions;

    use crate::{
        ir_io::{self, ReadError},
        typst_render::{self, RenderError},
    };

    let file = fs::File::open(&paths.input).map_err(|_| EngineFailure::io())?;
    let document = ir_io::read(BufReader::new(file), limits).map_err(|error| match error {
        ReadError::ByteLimitExceeded { .. }
        | ReadError::DepthExceeded(_)
        | ReadError::BlockLimitExceeded { .. } => EngineFailure::limit_exceeded(),
        ReadError::Io(_) => EngineFailure::io(),
        ReadError::Json(_) | ReadError::UnsupportedVersion => EngineFailure::invalid_request(),
    })?;

    let options = TypstOptions {
        title_fallback: settings.title_fallback.clone(),
        date: Some(settings.date),
        ..TypstOptions::default()
    };
    let rendered = typst_render::render(&document, &options, settings.timestamp).map_err(
        |error| match error {
            RenderError::InvalidTimestamp => EngineFailure::invalid_request(),
            RenderError::Compile { .. } | RenderError::Export { .. } => EngineFailure::new(
                ErrorCode::EngineFailure,
                "Typst could not typeset the document.",
            ),
        },
    )?;
    drop(document);
    if limits
        .max_pages
        .is_some_and(|limit| rendered.page_count as u64 > u64::from(limit))
    {
        return Err(EngineFailure::limit_exceeded());
    }

    for warning in &rendered.writer_warnings {
        common::emit_warning(output, warning)?;
    }
    if !rendered.typst_warnings.is_empty() {
        emit(
            output,
            "typst_warning",
            format!(
                "The Typst compiler reported {} warning(s) while typesetting the document.",
                rendered.typst_warnings.len()
            ),
        )?;
    }
    if rendered.missing_glyphs > 0 {
        emit(
            output,
            "glyph_missing",
            format!(
                "{} character(s) have no glyph in the embedded fonts and render as empty boxes.",
                rendered.missing_glyphs
            ),
        )?;
    }

    let artifact = paths.output_dir.join("document.pdf");
    fs::write(&artifact, &rendered.bytes).map_err(|_| EngineFailure::io())?;
    common::emit_success(output, &artifact, Format::Pdf.id())
}

#[cfg(feature = "typst")]
fn emit<W: Write>(output: &mut W, code: &str, message: String) -> Result<(), EngineFailure> {
    common::emit(
        output,
        Event::Warning {
            code: code.to_owned(),
            message,
        },
    )
    .map_err(|_| EngineFailure::io())
}

#[cfg(not(feature = "typst"))]
fn convert<W: Write>(
    _paths: &common::ConvertPaths,
    _limits: &ariad_core::limits::Limits,
    _settings: &Settings,
    _output: &mut W,
) -> Result<(), EngineFailure> {
    Err(EngineFailure::new(
        ErrorCode::ToolMissing,
        "This ashift build does not include the Typst PDF writer.",
    ))
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, io::Cursor, path::Path};

    use ariad_core::protocol::{ErrorCode, Event, PROTOCOL, Request, ToolAvailability};
    use serde_json::{Value, json};

    use super::{Settings, TYPST_VERSION, calendar_date};

    #[test]
    fn describe_reports_the_typst_route_and_version() {
        let request = Request::Describe {
            protocol: PROTOCOL.to_owned(),
            job: "describe".to_owned(),
        };
        let mut output = Vec::new();
        let exit = super::serve(
            Cursor::new(serde_json::to_vec(&request).unwrap()),
            &mut output,
        );
        assert_eq!(exit, std::process::ExitCode::SUCCESS);
        let first: Event =
            serde_json::from_slice(output.split(|b| *b == b'\n').next().unwrap()).unwrap();
        let Event::Capabilities {
            engine,
            tool,
            license,
            routes,
            enforces_memory_limit,
            ..
        } = first
        else {
            panic!("expected capabilities");
        };
        assert_eq!(engine, "typst");
        assert_eq!(tool.name, "typst");
        assert_eq!(tool.version.as_deref(), Some(TYPST_VERSION));
        let expected = if cfg!(feature = "typst") {
            ToolAvailability::Found
        } else {
            ToolAvailability::Missing
        };
        assert_eq!(tool.status, expected);
        assert_eq!(license, "Apache-2.0");
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].input, "ariad-ir+json");
        assert_eq!(routes[0].output, "pdf");
        assert!(!enforces_memory_limit);
    }

    #[test]
    fn typst_version_matches_the_workspace_lockfile() {
        let lockfile = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
        let lockfile = std::fs::read_to_string(lockfile).unwrap();
        let locked: Vec<&str> = lockfile
            .split("[[package]]")
            .filter(|package| package.lines().any(|line| line == "name = \"typst\""))
            .filter_map(|package| {
                package
                    .lines()
                    .find_map(|line| line.strip_prefix("version = \""))
                    .and_then(|version| version.strip_suffix('"'))
            })
            .collect();
        assert_eq!(locked, [TYPST_VERSION]);
    }

    #[test]
    fn options_are_validated_at_the_boundary() {
        let options =
            |pairs: Value| -> BTreeMap<String, Value> { serde_json::from_value(pairs).unwrap() };
        let settings = Settings::from_options(&options(json!({
            "title_fallback": "Báo cáo",
            "timestamp": 1_700_000_000,
            "profile": "editable",
        })))
        .unwrap();
        assert_eq!(settings.title_fallback.as_deref(), Some("Báo cáo"));
        assert_eq!(settings.timestamp, 1_700_000_000);
        assert_eq!(settings.date, (2023, 11, 14));

        let defaulted = Settings::from_options(&BTreeMap::new()).unwrap();
        assert!(
            defaulted.timestamp > 1_700_000_000,
            "falls back to the clock, not zero"
        );

        for invalid in [
            json!({"timestamp": "1700000000"}),
            json!({"timestamp": 1.5}),
            json!({"timestamp": i64::MAX}),
            json!({"title_fallback": 3}),
        ] {
            let error = Settings::from_options(&options(invalid)).unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidRequest);
        }
    }

    #[test]
    fn calendar_dates_stay_within_typst_years() {
        assert_eq!(calendar_date(0), Some((1970, 1, 1)));
        assert_eq!(calendar_date(951_782_400), Some((2000, 2, 29)));
        assert_eq!(calendar_date(-62_135_596_800), Some((1, 1, 1)));
        assert_eq!(calendar_date(-62_135_596_801), None);
        assert_eq!(calendar_date(253_402_300_800), None);
    }
}

use std::io::{self, Write};

use ariad_host::commands::inspect::InspectOutput;

/// Renders inspection output as pretty-printed JSON to stdout.
pub fn render_inspect_json(output: &InspectOutput) -> io::Result<()> {
    let json_bytes = serde_json::to_vec_pretty(output).map_err(io::Error::other)?;
    let mut stdout = io::stdout().lock();
    stdout.write_all(&json_bytes)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

fn plural(n: usize, singular: &str, plural_str: &str) -> String {
    if n == 1 {
        format!("{n} {singular}")
    } else {
        format!("{n} {plural_str}")
    }
}

/// Escapes terminal control characters and line separators in strings destined for human output.
pub fn escape_control_chars(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_control()
            || ('\u{0080}'..='\u{009F}').contains(&c)
            || c == '\u{2028}'
            || c == '\u{2029}'
        {
            for ec in c.escape_default() {
                out.push(ec);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Renders inspection output as human-readable text to stdout.
pub fn render_inspect_human(output: &InspectOutput) -> io::Result<()> {
    let mut out = io::stdout().lock();
    writeln!(out, "format:    {}", output.format.id())?;
    writeln!(out, "bytes:     {}", output.bytes)?;
    if let Some(pages) = output.page_count {
        writeln!(out, "pages:     {pages}")?;
    }
    if let Some(ref meta) = output.meta {
        if let Some(ref title) = meta.title {
            writeln!(out, "title:     {}", escape_control_chars(title))?;
        }
        if !meta.authors.is_empty() {
            let authors: Vec<String> = meta
                .authors
                .iter()
                .map(|a| escape_control_chars(a))
                .collect();
            writeln!(out, "authors:   {}", authors.join(", "))?;
        }
        if let Some(ref date) = meta.date {
            writeln!(out, "date:      {}", escape_control_chars(date))?;
        }
        if let Some(ref lang) = meta.language {
            writeln!(out, "language:  {}", escape_control_chars(lang))?;
        }
    }
    if let Some(ref counts) = output.counts {
        let total_headings: usize = counts.headings.values().sum();
        let heading_detail = if total_headings > 0 {
            let parts: Vec<String> = counts
                .headings
                .iter()
                .map(|(lvl, count)| format!("h{lvl}: {count}"))
                .collect();
            format!(" ({})", parts.join(", "))
        } else {
            String::new()
        };
        let heading_label = if total_headings == 1 {
            "heading"
        } else {
            "headings"
        };
        let p_str = plural(counts.paragraphs, "paragraph", "paragraphs");
        let t_str = plural(counts.tables, "table", "tables");
        let img_str = plural(counts.images, "image", "images");
        let lk_str = plural(counts.links, "link", "links");
        let fn_str = plural(counts.footnotes, "footnote", "footnotes");
        let w_str = plural(counts.words, "word", "words");
        writeln!(
            out,
            "counts:    {total_headings} {heading_label}{heading_detail}, {p_str}, {t_str}, {img_str}, {lk_str}, {fn_str}, {w_str}",
        )?;
    }
    if output.reachable.is_empty() {
        writeln!(out, "reachable: (none)")?;
    } else {
        let targets: Vec<&'static str> = output.reachable.iter().map(|f| f.id()).collect();
        writeln!(out, "reachable: {}", targets.join(", "))?;
    }
    out.flush()
}

/// Renders plan output as pretty-printed JSON to stdout.
pub fn render_plan_json(output: &ariad_host::commands::plan::PlanOutput) -> io::Result<()> {
    let json_bytes = serde_json::to_vec_pretty(output).map_err(io::Error::other)?;
    let mut stdout = io::stdout().lock();
    stdout.write_all(&json_bytes)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

/// Returns the status check mark string appropriate for the platform and terminal.
pub fn check_mark() -> &'static str {
    if cfg!(windows) { "[ok]" } else { "✓" }
}

fn format_route(steps: &[ariad_host::commands::plan::PlanStep]) -> String {
    if steps.is_empty() {
        return "(no route)".to_owned();
    }
    let mut s = String::new();
    for (i, step) in steps.iter().enumerate() {
        if i == 0 {
            s.push_str(step.from.id());
        }
        s.push_str(&format!(" ─{}→ {}", step.engine, step.to.id()));
    }
    s
}

fn format_score(
    score: &ariad_host::commands::plan::PlanScore,
    measured: bool,
    runs_locally: bool,
    missing_engine: Option<&str>,
) -> String {
    let check = check_mark();
    let local_suffix = if runs_locally {
        format!("runs locally {check}")
    } else if let Some(engine) = missing_engine {
        format!("engine missing ({})", escape_control_chars(engine))
    } else {
        "engine missing".to_owned()
    };
    if !measured {
        return format!("unmeasured · {local_suffix}");
    }
    let mut parts = Vec::new();
    if let Some(f) = score.fidelity {
        parts.push(format!("fidelity {f:.2}"));
    }
    if let Some(e) = score.editability {
        parts.push(format!("editability {e:.2}"));
    }
    if let Some(duration) = score.estimated_duration_ms {
        if duration < 1000.0 {
            parts.push(format!("estimated {duration:.0}ms"));
        } else {
            let s = duration / 1000.0;
            if s.fract() < 0.05 {
                parts.push(format!("estimated {s:.0}s"));
            } else {
                parts.push(format!("estimated {s:.1}s"));
            }
        }
    }
    parts.push(local_suffix);
    parts.join(" · ")
}

/// Renders plan output as human-readable text to stdout.
pub fn render_plan_human(output: &ariad_host::commands::plan::PlanOutput) -> io::Result<()> {
    let mut out = io::stdout().lock();
    let input_str = escape_control_chars(output.input_summary.as_deref().unwrap_or(&output.input));
    writeln!(out, "input   {input_str}")?;
    writeln!(out, "route   {}", format_route(&output.route))?;
    writeln!(
        out,
        "score   {}",
        format_score(
            &output.score,
            output.measured,
            output.runs_locally,
            output.missing_engine.as_deref()
        )
    )?;
    for alt in &output.alternatives {
        writeln!(
            out,
            "alt     {} · {}",
            format_route(&alt.route),
            format_score(
                &alt.score,
                alt.measured,
                alt.runs_locally,
                alt.missing_engine.as_deref()
            )
        )?;
    }
    out.flush()
}

/// Renders engines output as pretty-printed JSON to stdout.
pub fn render_engines_json(rows: &[ariad_host::commands::engines::EngineRow]) -> io::Result<()> {
    let json_bytes = serde_json::to_vec_pretty(rows).map_err(io::Error::other)?;
    let mut stdout = io::stdout().lock();
    stdout.write_all(&json_bytes)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

/// Renders engines output as human-readable table to stdout.
pub fn render_engines_human(rows: &[ariad_host::commands::engines::EngineRow]) -> io::Result<()> {
    let mut out = io::stdout().lock();
    writeln!(
        out,
        "{:<12} {:<9} {:<18} {:<34} ROUTES",
        "ENGINE", "VERSION", "LICENSE", "STATUS"
    )?;
    for row in rows {
        let routes_str = if row.routes.is_empty() {
            "(none)".to_owned()
        } else {
            row.routes.join(", ")
        };
        let status_str = match &row.note {
            Some(note) => format!("{} ({note})", row.status.as_str()),
            None => row.status.as_str().to_owned(),
        };
        writeln!(
            out,
            "{:<12} {:<9} {:<18} {:<34} {}",
            row.id, row.version, row.license, status_str, routes_str
        )?;
    }
    out.flush()
}

/// Returns the failure icon: ✗ or [x]
pub fn fail_icon() -> &'static str {
    if cfg!(windows) { "[x]" } else { "✗" }
}

/// Renders doctor output as pretty-printed JSON to stdout.
pub fn render_doctor_json(report: &ariad_host::commands::doctor::DoctorReport) -> io::Result<()> {
    let json_bytes = serde_json::to_vec_pretty(report).map_err(io::Error::other)?;
    let mut stdout = io::stdout().lock();
    stdout.write_all(&json_bytes)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

/// Renders doctor output as human-readable report to stdout.
pub fn render_doctor_human(report: &ariad_host::commands::doctor::DoctorReport) -> io::Result<()> {
    let mut out = io::stdout().lock();
    for check in &report.checks {
        let icon = if check.ok { check_mark() } else { fail_icon() };
        if check.id == "pandoc_version"
            && check.ok
            && let Some(ref path) = report.tool_path
        {
            writeln!(
                out,
                "{icon} {} at {}",
                check.message,
                escape_control_chars(&path.display().to_string())
            )?;
        } else {
            writeln!(out, "{icon} {}", check.message)?;
        }
        if !check.ok && !check.hint.is_empty() {
            writeln!(out, "  hint: {}", check.hint)?;
        }
    }
    out.flush()
}

/// Renders convert output as pretty-printed JSON to stdout.
pub fn render_convert_json(report: &ariad_host::convert::ConvertReport) -> io::Result<()> {
    let output = ariad_host::convert::ConvertOutput::from(report);
    let json_bytes = serde_json::to_vec_pretty(&output).map_err(io::Error::other)?;
    let mut stdout = io::stdout().lock();
    stdout.write_all(&json_bytes)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

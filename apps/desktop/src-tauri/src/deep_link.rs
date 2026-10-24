use crate::types::{DeepLink, Profile};

pub(crate) fn input_format(value: &str) -> Option<ariad_core::format::Format> {
    ariad_core::format::Format::from_user_name(value)
        .or_else(|| (value == "docling+json").then_some(ariad_core::format::Format::DoclingJson))
}

pub(crate) fn output_format(value: &str) -> Option<ariad_core::format::Format> {
    ariad_core::format::Format::from_user_name(value).filter(|format| {
        ariad_host::convert::executor_edges()
            .iter()
            .any(|(_, output, _)| output == format)
    })
}

pub(crate) fn format_name(format: ariad_core::format::Format) -> &'static str {
    match format {
        ariad_core::format::Format::Markdown => "md",
        ariad_core::format::Format::AriadIrJson => "json",
        other => other.id(),
    }
}

/// Parses only routing preferences. Deep links never carry filesystem paths or credentials.
pub fn parse(value: &str) -> Result<Option<DeepLink>, String> {
    let u = url::Url::parse(value).map_err(|_| "Invalid desktop link")?;
    if u.scheme() != "ariadshift"
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port().is_some()
        || u.fragment().is_some()
    {
        return Err("Invalid desktop link".into());
    }
    if u.host_str() == Some("auth") && u.path() == "/callback" {
        return Ok(None);
    }
    if u.host_str() != Some("open") || !matches!(u.path(), "" | "/") {
        return Err("Unsupported desktop link".into());
    }
    let mut route = None;
    let mut profile = None;
    for (key, value) in u.query_pairs() {
        match key.as_ref() {
            "route" if route.is_none() => route = Some(value.into_owned()),
            "profile" if profile.is_none() => profile = Some(value.into_owned()),
            _ => return Err("Unknown or duplicate link parameter".into()),
        }
    }
    let route = route.ok_or("Missing route")?;
    let (from, target) = route.split_once('-').ok_or("Invalid route")?;
    let from = input_format(from).ok_or("Unsupported input format")?;
    let target = output_format(target).ok_or("Unsupported target format")?;
    if from == target {
        return Err("Input and output formats must differ".into());
    }
    let profile = match profile.as_deref().unwrap_or("editable") {
        "editable" => Profile::Editable,
        "faithful" => Profile::Faithful,
        "fast" => Profile::Fast,
        "private" => Profile::Private,
        _ => return Err("Invalid profile".into()),
    };
    Ok(Some(DeepLink {
        from: format_name(from).into(),
        target: format_name(target).into(),
        profile,
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn links_only_select_preferences() {
        let link = parse("ariadshift://open?route=pdf-docx&profile=private")
            .unwrap()
            .unwrap();
        assert_eq!(link.from, "pdf");
        assert_eq!(link.profile, Profile::Private);
        for (route, expected) in [
            ("jpg-html", "jpeg"),
            ("tif-docx", "tiff"),
            ("webp-md", "webp"),
            ("odt-pdf", "odt"),
            ("xls-xlsx", "xls"),
            ("md-pdf", "md"),
            ("md-json", "md"),
            ("docling%2Bjson-html", "docling+json"),
        ] {
            assert_eq!(
                parse(&format!("ariadshift://open?route={route}"))
                    .unwrap()
                    .unwrap()
                    .from,
                expected
            );
        }
        assert!(
            parse("ariadshift://auth/callback?code=reserved")
                .unwrap()
                .is_none()
        );
        for bad in [
            "https://open?route=md-html",
            "ariadshift://open?route=md-png",
            "ariadshift://open?route=unknown-html",
            "ariadshift://open?route=md-html&path=/private",
            "ariadshift://open?route=md-html&route=md-docx",
            "ariadshift://open?route=md-html&profile=invalid",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}

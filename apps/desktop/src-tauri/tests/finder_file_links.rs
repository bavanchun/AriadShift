use ariad_desktop::{deep_link, file_open, types::Profile};

#[test]
fn finder_file_urls_belong_to_file_open() {
    let directory = tempfile::tempdir().unwrap();
    let document = directory.path().join("Report Q3.md");
    std::fs::write(&document, "# Report").unwrap();
    let encoded = url::Url::from_file_path(&document).unwrap();
    assert!(encoded.as_str().contains("Report%20Q3.md"));
    let mut local = encoded.clone();
    local.set_host(Some("localhost")).unwrap();
    for url in [encoded, local] {
        assert!(deep_link::parse(url.as_str()).unwrap().is_none(), "{url}");
        assert_eq!(
            file_open::parse_urls(&[url]),
            [document.to_str().unwrap().to_owned()]
        );
    }
    for url in [
        "file:///Users/me/Report%20Q3.docx",
        "file://localhost/Users/me/notes.md",
    ] {
        assert!(deep_link::parse(url).unwrap().is_none(), "{url}");
    }
}

#[test]
fn desktop_links_still_validate_routing_preferences() {
    let link = deep_link::parse("ariadshift://open?route=md-html&profile=private")
        .unwrap()
        .unwrap();
    assert_eq!((link.from.as_str(), link.target.as_str()), ("md", "html"));
    assert_eq!(link.profile, Profile::Private);
    assert!(
        deep_link::parse("ariadshift://auth/callback?code=reserved")
            .unwrap()
            .is_none()
    );
    for invalid in [
        "not a URL",
        "https://open?route=md-html",
        "unknown://open?route=md-html",
        "ariadshift://unknown?route=md-html",
        "ariadshift://open?route=md-png",
        "ariadshift://open?route=md-html&path=/private",
        "ariadshift://open?route=md-html&route=md-docx",
    ] {
        assert!(deep_link::parse(invalid).is_err(), "{invalid}");
    }
}

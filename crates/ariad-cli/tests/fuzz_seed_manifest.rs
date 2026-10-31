mod support;

use support::fixtures::{load_manifest, validate_fuzz_seed_manifest, validate_manifest};

#[test]
fn manifest_checker_validates_all_fuzz_seed_hashes_and_provenance() {
    let manifest = load_manifest().unwrap();
    assert!(
        !manifest.fuzz_seed.is_empty(),
        "the fuzz corpus must be inventoried"
    );
    // The document checker temporarily creates an unlisted document in another test.
    // Validate the independent seed collection without observing that transient fixture.
    assert!(validate_fuzz_seed_manifest(&manifest).is_empty());

    let mut invalid = manifest.clone();
    invalid.fuzz_seed[0].sha256 = "0".repeat(64);
    assert!(
        validate_fuzz_seed_manifest(&invalid)
            .iter()
            .any(|e| e.contains("fuzz seed SHA-256 mismatch"))
    );
    // Also prove that the combined manifest checker invokes the seed validator.
    assert!(
        validate_manifest(&invalid)
            .iter()
            .any(|e| e.contains("fuzz seed SHA-256 mismatch"))
    );
    let mut invalid = manifest.clone();
    invalid.fuzz_seed[0].license = "GPL-3.0-only".to_owned();
    assert!(
        validate_fuzz_seed_manifest(&invalid)
            .iter()
            .any(|e| e.contains("fuzz seed") && e.contains("disallowed license"))
    );
    let mut invalid = manifest.clone();
    invalid.fuzz_seed[0].source.clear();
    assert!(
        validate_fuzz_seed_manifest(&invalid)
            .iter()
            .any(|e| e.contains("missing provenance"))
    );
    let mut invalid = manifest.clone();
    invalid.fuzz_seed.remove(0);
    assert!(
        validate_fuzz_seed_manifest(&invalid)
            .iter()
            .any(|e| e.contains("fuzz seed has no manifest entry"))
    );
    let mut invalid = manifest.clone();
    invalid.fuzz_seed.push(invalid.fuzz_seed[0].clone());
    let errors = validate_fuzz_seed_manifest(&invalid);
    assert!(errors.iter().any(|e| e.contains("duplicate fuzz seed id")));
    assert!(
        errors
            .iter()
            .any(|e| e.contains("duplicate fuzz seed path"))
    );
    for path in [
        "fuzz/seeds/../outside",
        "fixtures/md/en-footnotes.md",
        "/tmp/seed",
        "fuzz/seeds/wrong-target/seed",
    ] {
        let mut invalid = manifest.clone();
        invalid.fuzz_seed[0].path = path.to_owned();
        assert!(
            validate_fuzz_seed_manifest(&invalid)
                .iter()
                .any(|e| e.contains("unsafe fuzz seed path or target"))
        );
    }
}

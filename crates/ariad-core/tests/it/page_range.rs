use ariad_core::tools::{DuplicatePolicy, PageRange, PageRangeError};

#[test]
fn expands_all_normative_grammar_forms() {
    let cases: &[(&str, u32, DuplicatePolicy, &[u32])] = &[
        (
            "all",
            10,
            DuplicatePolicy::Reject,
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        ),
        ("even", 10, DuplicatePolicy::Reject, &[2, 4, 6, 8, 10]),
        ("odd", 10, DuplicatePolicy::Reject, &[1, 3, 5, 7, 9]),
        ("5", 10, DuplicatePolicy::Reject, &[5]),
        ("2-5", 10, DuplicatePolicy::Reject, &[2, 3, 4, 5]),
        ("5-2", 10, DuplicatePolicy::Reject, &[5, 4, 3, 2]),
        ("8-", 10, DuplicatePolicy::Reject, &[8, 9, 10]),
        ("-3", 10, DuplicatePolicy::Reject, &[1, 2, 3]),
        ("last", 10, DuplicatePolicy::Reject, &[10]),
        ("last-1", 10, DuplicatePolicy::Reject, &[9]),
        ("last-2-last", 10, DuplicatePolicy::Reject, &[8, 9, 10]),
        (
            "1-3, 5, last-1",
            10,
            DuplicatePolicy::Reject,
            &[1, 2, 3, 5, 9],
        ),
        (
            "1,1,5-3,last",
            10,
            DuplicatePolicy::KeepInOrder,
            &[1, 1, 5, 4, 3, 10],
        ),
        ("last-", 10, DuplicatePolicy::Reject, &[10]),
        ("last-2-", 10, DuplicatePolicy::Reject, &[8, 9, 10]),
        ("-last", 4, DuplicatePolicy::Reject, &[1, 2, 3, 4]),
        ("-last-2", 5, DuplicatePolicy::Reject, &[1, 2, 3]),
        ("last-last-2", 5, DuplicatePolicy::Reject, &[5, 4, 3]),
        ("last-1-last-3", 6, DuplicatePolicy::Reject, &[5, 4, 3]),
        (
            "  2-4 , 7 , last  ",
            8,
            DuplicatePolicy::Reject,
            &[2, 3, 4, 7, 8],
        ),
    ];

    for &(expr, count, policy, expected) in cases {
        let range =
            PageRange::parse(expr).unwrap_or_else(|e| panic!("failed to parse {expr:?}: {e}"));
        let actual = range
            .expand(count, policy)
            .unwrap_or_else(|e| panic!("failed to expand {expr:?} against {count}: {e}"));
        assert_eq!(actual, expected, "mismatch for expression {expr:?}");
    }
}

#[test]
fn duplicate_policy_keep_in_order_vs_reject() {
    let range = PageRange::parse("1-4,3-5").expect("valid range");
    assert_eq!(
        range.expand(10, DuplicatePolicy::KeepInOrder).unwrap(),
        vec![1, 2, 3, 4, 3, 4, 5]
    );

    let err = range.expand(10, DuplicatePolicy::Reject).unwrap_err();
    assert_eq!(
        err,
        PageRangeError::DuplicatePage {
            page: 3,
            token: "3-5".to_owned(),
        }
    );
    assert_eq!(err.token(), "3-5");
    assert_eq!(
        err.to_string(),
        r#"duplicate page 3 in page range token "3-5" (already selected)"#
    );
}

#[test]
fn normative_error_messages_match_docs_tools_specification() {
    // Syntax error
    let err = PageRange::parse("1..3").unwrap_err();
    assert_eq!(
        err,
        PageRangeError::InvalidToken {
            token: "1..3".to_owned()
        }
    );
    assert_eq!(
        err.to_string(),
        r#"invalid page range token "1..3": expected page number, range, "all", "even", "odd", or "last""#
    );

    // Zero index
    for zero_tok in ["0", "0-3", "3-0", "-0", "0-"] {
        let err = PageRange::parse(zero_tok).unwrap_err();
        assert_eq!(
            err,
            PageRangeError::ZeroIndex {
                token: zero_tok.to_owned()
            }
        );
        assert_eq!(
            err.to_string(),
            format!(r#"invalid page range token "{zero_tok}": pages are 1-based"#)
        );
    }

    // Invalid leading zeros and malformed tokens
    for invalid in [
        "", "   ", "1,", ",1", "1,,2", "01", "last-0", "last-01", "1-2-3", "foo", "1 - 2",
    ] {
        assert!(
            PageRange::parse(invalid).is_err(),
            "expected {invalid:?} to fail parsing"
        );
    }

    // Out-of-bounds page
    let err = PageRange::parse("1-3, 12-15")
        .unwrap()
        .expand(10, DuplicatePolicy::Reject)
        .unwrap_err();
    assert_eq!(
        err,
        PageRangeError::ExceedsPageCount {
            token: "12-15".to_owned(),
            page_count: 10
        }
    );
    assert_eq!(
        err.to_string(),
        r#"page range token "12-15" exceeds document page count (10)"#
    );

    // Out-of-bounds last-K
    let err = PageRange::parse("1, last-10")
        .unwrap()
        .expand(10, DuplicatePolicy::Reject)
        .unwrap_err();
    assert_eq!(
        err,
        PageRangeError::UnderflowsPageCount {
            token: "last-10".to_owned(),
            page_count: 10
        }
    );
    assert_eq!(
        err.to_string(),
        r#"page range token "last-10" underflows document page count (10)"#
    );

    // Empty selection (even on a 1-page PDF)
    let err = PageRange::parse("even")
        .unwrap()
        .expand(1, DuplicatePolicy::Reject)
        .unwrap_err();
    assert_eq!(
        err,
        PageRangeError::EmptySelection {
            token: "even".to_owned(),
            page_count: 1
        }
    );
}

#[test]
fn property_invariants_hold_across_document_lengths() {
    for page_count in 1..=35u32 {
        // 1. "all" expands to 1..=page_count
        let all_pages = PageRange::all()
            .expand(page_count, DuplicatePolicy::Reject)
            .unwrap();
        let expected_all: Vec<u32> = (1..=page_count).collect();
        assert_eq!(all_pages, expected_all);

        // 2. "odd" and "even" partition 1..=page_count
        let odd_pages = PageRange::parse("odd")
            .unwrap()
            .expand(page_count, DuplicatePolicy::Reject)
            .unwrap();
        assert!(
            odd_pages
                .iter()
                .all(|&p| p >= 1 && p <= page_count && p % 2 == 1)
        );
        if page_count >= 2 {
            let even_pages = PageRange::parse("even")
                .unwrap()
                .expand(page_count, DuplicatePolicy::Reject)
                .unwrap();
            assert!(
                even_pages
                    .iter()
                    .all(|&p| p >= 1 && p <= page_count && p % 2 == 0)
            );
            assert_eq!(odd_pages.len() + even_pages.len(), page_count as usize);
        }

        // 3. Reverse range symmetry and open-ended equivalence for sample endpoints
        for a in [1, (page_count / 2).max(1), page_count] {
            for b in [1, (page_count / 3).max(1), page_count] {
                let forward = PageRange::parse(&format!("{a}-{b}"))
                    .unwrap()
                    .expand(page_count, DuplicatePolicy::Reject)
                    .unwrap();
                let mut backward = PageRange::parse(&format!("{b}-{a}"))
                    .unwrap()
                    .expand(page_count, DuplicatePolicy::Reject)
                    .unwrap();
                backward.reverse();
                assert_eq!(
                    forward, backward,
                    "symmetry failed for {a}-{b} on {page_count} pages"
                );

                // Unique range produces identical output under KeepInOrder and Reject
                let keep = PageRange::parse(&format!("{a}-{b}"))
                    .unwrap()
                    .expand(page_count, DuplicatePolicy::KeepInOrder)
                    .unwrap();
                assert_eq!(forward, keep);
            }

            let open_end = PageRange::parse(&format!("{a}-"))
                .unwrap()
                .expand(page_count, DuplicatePolicy::Reject)
                .unwrap();
            let explicit_end = PageRange::parse(&format!("{a}-last"))
                .unwrap()
                .expand(page_count, DuplicatePolicy::Reject)
                .unwrap();
            assert_eq!(open_end, explicit_end);

            let open_start = PageRange::parse(&format!("-{a}"))
                .unwrap()
                .expand(page_count, DuplicatePolicy::Reject)
                .unwrap();
            let explicit_start = PageRange::parse(&format!("1-{a}"))
                .unwrap()
                .expand(page_count, DuplicatePolicy::Reject)
                .unwrap();
            assert_eq!(open_start, explicit_start);
        }
    }
}

use std::str::FromStr;

use ariad_core::tools::{
    FREE_MAX_RUN_BYTES, FREE_MAX_RUN_FILES, Plan, RunSize, Tier, ToolAccess, allowed, find,
    within_run_quota,
};

#[test]
fn tier_and_plan_matrix_matches_access_specification() {
    assert_eq!(Plan::default(), Plan::Free);
    assert_eq!(ToolAccess::local_default(), ToolAccess::FREE);

    for &plan in Plan::ALL {
        let parsed = Plan::from_str(plan.as_str()).expect("parse plan");
        assert_eq!(parsed, plan);
        assert_eq!(plan.to_string(), plan.as_str());

        let access = ToolAccess::for_plan(plan);
        assert_eq!(access.plan, plan);
        assert!(allowed(Tier::Free, plan));
        assert!(access.allows(Tier::Free));

        let expected_pro = matches!(plan, Plan::Pro | Plan::Team | Plan::Enterprise);
        assert_eq!(plan.grants_pro(), expected_pro);
        assert_eq!(access.grants_pro(), expected_pro);
        assert_eq!(allowed(Tier::Pro, plan), expected_pro);
        assert_eq!(access.allows(Tier::Pro), expected_pro);
    }

    assert!(Plan::from_str("unknown").is_err());

    let pdf_info = find("pdf.info").expect("pdf.info registered");
    assert!(ToolAccess::FREE.allows_spec(pdf_info));
    assert!(ToolAccess::PRO.allows_spec(pdf_info));
}

#[test]
fn within_run_quota_enforces_free_plan_file_and_byte_limits() {
    assert_eq!(FREE_MAX_RUN_FILES, 25);
    assert_eq!(FREE_MAX_RUN_BYTES, 250 * 1_000_000);

    // Free plan: within limits and at exact boundary (25 files, 250 MB)
    assert!(within_run_quota(Plan::Free, RunSize::new(0, 0)));
    assert!(within_run_quota(Plan::Free, RunSize::new(1, 1024)));
    assert!(within_run_quota(
        Plan::Free,
        RunSize::new(FREE_MAX_RUN_FILES, FREE_MAX_RUN_BYTES)
    ));
    assert!(
        ToolAccess::FREE.within_run_quota(RunSize::new(FREE_MAX_RUN_FILES, FREE_MAX_RUN_BYTES))
    );

    // Free plan: just over the file count limit (26 files)
    assert!(!within_run_quota(
        Plan::Free,
        RunSize::new(FREE_MAX_RUN_FILES + 1, 0)
    ));
    assert!(!within_run_quota(
        Plan::Free,
        RunSize::new(FREE_MAX_RUN_FILES + 1, FREE_MAX_RUN_BYTES)
    ));

    // Free plan: just over the byte size limit (250_000_001 bytes)
    assert!(!within_run_quota(
        Plan::Free,
        RunSize::new(1, FREE_MAX_RUN_BYTES + 1)
    ));
    assert!(!within_run_quota(
        Plan::Free,
        RunSize::new(FREE_MAX_RUN_FILES, FREE_MAX_RUN_BYTES + 1)
    ));

    // Pro, Team, and Enterprise plans: unlimited files and bytes
    for plan in [Plan::Pro, Plan::Team, Plan::Enterprise] {
        let access = ToolAccess::for_plan(plan);
        for size in [
            RunSize::new(FREE_MAX_RUN_FILES + 1, 0),
            RunSize::new(1, FREE_MAX_RUN_BYTES + 1),
            RunSize::new(FREE_MAX_RUN_FILES + 1, FREE_MAX_RUN_BYTES + 1),
            RunSize::new(10_000, 100 * 1_000_000_000),
            RunSize::new(usize::MAX, u64::MAX),
        ] {
            assert!(
                within_run_quota(plan, size),
                "expected {plan} to allow {size:?}"
            );
            assert!(access.within_run_quota(size));
        }
    }
}

pub mod color_snapshot;
pub mod marks;
pub mod page_set_hostile;

/// Requires that a child run of this test binary (`--exact <module>::<test>`) executed exactly
/// one test. A name without its module path matches nothing in the merged binary, and a child
/// that matches nothing exits 0 and would pass silently. Only unix tests with the `codecs`
/// feature re-run themselves, so other configurations never call it.
#[allow(dead_code)]
pub fn assert_ran_one_test(stdout: &str, name: &str) {
    assert!(
        stdout.contains("running 1 test") && stdout.contains("1 passed; 0 failed"),
        "the child must run exactly the one test {name}, got: {stdout}"
    );
}

use std::path::PathBuf;

use openagent::trust::policy_test::run_policy_tests;

#[test]
fn policy_golden_cases_are_stable() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let policy_path = root.join("tests/fixtures/policy/golden_policy.yaml");
    let cases_path = root.join("tests/fixtures/policy/golden_policy_cases.yaml");

    let report = run_policy_tests(&policy_path, &cases_path).expect("run policy golden cases");
    let failures = report
        .cases
        .iter()
        .filter(|c| !c.pass)
        .map(|c| format!("{}: {}", c.name, c.failures.join("; ")))
        .collect::<Vec<_>>();

    assert_eq!(
        report.failed,
        0,
        "policy golden drift detected:\n{}",
        failures.join("\n")
    );
    assert_eq!(
        report.passed,
        report.cases.len(),
        "expected all policy golden cases to pass"
    );
}


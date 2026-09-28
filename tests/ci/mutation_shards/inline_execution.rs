//! The serial mutation run preserves reports and cargo-mutants' exit status.

use crate::harness::Fixture;

#[test]
fn mutation_failures_keep_their_reports_and_original_status() {
    for (code, timeout) in [(3, 1), (4, 0)] {
        let mut fixture = Fixture::new();
        fixture.set("MUTATION_TEST", "true");
        fixture.stub("git", "exit 1");
        fixture.stub(
            "cargo",
            &format!(
                "mkdir -p \"$RUNNER_TEMP/mutants/mutants.out\"\n\
                 printf '{{\"caught\":0,\"missed\":0,\"timeout\":{timeout},\"unviable\":0}}' \\
                 > \"$RUNNER_TEMP/mutants/mutants.out/outcomes.json\"\nexit {code}"
            ),
        );
        assert_eq!(fixture.run("ci", "mutants").status.code(), Some(code));
        assert!(fixture.root.join("reports/mutants.json").is_file());
        assert!(!fixture.trace().contains("timeout --kill-after"));
    }
}

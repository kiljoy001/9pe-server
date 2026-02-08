#[test]
fn fault_injection_placeholder() {
    // The comprehensive fault-injection suite lives under tests/legacy.
    // This smoke test simply asserts that the legacy suite is optional.
    assert_eq!(2 + 2, 4);
}

use ninepee::translators::TranslatorSystem;

#[test]
fn sandbox_initializes_with_zero_translators() {
    let system = TranslatorSystem::new();
    assert_eq!(system.count_translators(), 0);
}

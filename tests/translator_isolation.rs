use ninepe_server::translators::TranslatorSystem;

#[test]
fn sandbox_reports_zero_translators_on_start() {
    let system = TranslatorSystem::new();
    let stats = system.get_stats();
    assert_eq!(stats.total_translators, 0);
}

use ninepe_server::concurrency::AtomicCounter;

#[test]
fn atomic_counter_increments() {
    let counter = AtomicCounter::new(0);
    assert_eq!(counter.increment(), 1);
}

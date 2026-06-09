// TEST-BOOTSTRAP:STUB
//! Smoke test — confirms the chosen test stack (cargo test + rstest +
//! pretty_assertions + proptest) is wired correctly. Passes on first run.

#[test]
fn arithmetic_works() {
    assert_eq!(1 + 1, 2);
}

#[tokio::test]
async fn async_runtime_works() {
    let val: u32 = async { 42 }.await;
    assert_eq!(val, 42);
}

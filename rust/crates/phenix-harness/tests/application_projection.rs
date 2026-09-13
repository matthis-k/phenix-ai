#[path = "../src/application.rs"]
mod application;

#[test]
fn fixed_application_queue_capacities_match_the_runtime_contract() {
    assert_eq!(application::APPLICATION_INVOCATION_CAPACITY, 64);
    assert_eq!(application::CLIENT_CAPABILITY_CAPACITY, 64);
    assert_eq!(application::APPLICATION_EVENT_CAPACITY, 256);
}

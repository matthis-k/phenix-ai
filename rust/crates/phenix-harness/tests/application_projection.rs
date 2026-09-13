use phenix_harness::application::{
    APPLICATION_EVENT_CAPACITY, APPLICATION_INVOCATION_CAPACITY, CLIENT_CAPABILITY_CAPACITY,
};

#[test]
fn fixed_application_queue_capacities_match_the_runtime_contract() {
    assert_eq!(APPLICATION_INVOCATION_CAPACITY, 64);
    assert_eq!(CLIENT_CAPABILITY_CAPACITY, 64);
    assert_eq!(APPLICATION_EVENT_CAPACITY, 256);
}

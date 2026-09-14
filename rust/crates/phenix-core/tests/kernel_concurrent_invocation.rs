use phenix_core::Kernel;

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn kernel_supports_shared_immutable_invocation() {
    assert_send_sync::<Kernel>();
}

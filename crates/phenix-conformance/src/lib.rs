//! Declarative builders used only by native conformance fixtures.
use phenix_kernel::resolve::SlotBinding;
pub fn binding(caller: &str, key: &str, provider: &str) -> SlotBinding {
    let (slot, export) = match (caller, key) {
        ("example.cycle", "example.cycle@1/run") => ("Again", "Cycle"),
        ("example.other", "example.transform.apply@1/apply") => ("Provider", "Transform"),
        ("example.consumer", "example.transform.apply@1/apply") => ("Provider", "Transform"),
        ("example.pending", "example.transform.apply@1/apply") => ("Service", "Transform"),
        ("example.agent", "example.model@1/complete") => ("Model", "Complete"),
        ("example.agent", "example.tool.execute@1/execute") => ("Tool", "Execute"),
        ("example.lifecycle.a-consumer", "example.lifecycle-service@1/echo") => {
            ("Service", "LifecycleService")
        }
        ("example.relay", "example.resource.bump@1/bump") => ("Effect", "Bump"),
        ("example.choice", "example.transform.apply@1/apply") => ("Provider", "Transform"),
        ("example.choice", "example.transform.count@1/count") => ("OptionalCounter", "ReadCount"),
        ("example.restricted", "example.relay@1/run") => ("Delegate", "Relay"),
        ("example.restricted", "example.resource.bump@1/bump") => ("EffectLimit", "Bump"),
        ("example.pending-consumer", "example.pending@1/run") => ("Worker", "PendingRun"),
        ("example.pending-consumer", "example.transform.apply@1/apply") => {
            ("TransitiveEffect", "Transform")
        }
        _ => panic!("unknown fixture import {caller}/{key}"),
    };
    SlotBinding {
        caller: caller.into(),
        slot: slot.into(),
        index: 0,
        provider: provider.into(),
        export: export.into(),
    }
}

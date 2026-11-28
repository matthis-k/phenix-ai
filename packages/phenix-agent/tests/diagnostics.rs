//! CI developer build: select an ordinary fake model and optional diagnostics
//! plugin, then verify runtime observations rather than configuration prints.

use phenix_agent::{
    AgentEvent,
    diagnostics::{Diagnostics, TraceRecord},
    frontend::run_turn,
    harness::{self, Composition, Environment, Extension},
    model::{
        ModelProfile,
        fake::{FakeArguments, tool_roundtrip},
    },
    tools::Tool,
};
use phenix_graph::ContractValue;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

const LOGGER: &str = "dev-logger";

fn tools() -> Environment {
    Environment::new().with_tool(Tool::new("repeat", |args: FakeArguments| {
        if args.count != 2 {
            return Err("unexpected argument".into());
        }
        Ok(args.text.repeat(args.count as usize))
    }))
}

fn scenario(advanced: bool, logging: bool) -> (Vec<AgentEvent>, Vec<TraceRecord>) {
    let records = Arc::new(Mutex::new(Vec::new()));
    let extension = if logging {
        let records = records.clone();
        let logger = Diagnostics::new(LOGGER, move |record| {
            println!("[agent-trace] {record:?}");
            records.lock().unwrap().push(record);
        });
        let policy = logger.clone();
        vec![Extension::new(LOGGER, logger, move |options, model| {
            policy.configure(options, LOGGER, model.offer.clone(), &model.path);
            Ok(())
        })]
    } else {
        Vec::new()
    };
    let additional = if advanced {
        BTreeMap::from([("reference".into(), tools())])
    } else {
        BTreeMap::new()
    };
    let environment = if advanced { "reference" } else { "local" };
    let host = harness::compose(
        |context| tool_roundtrip(context.clone(), environment),
        tools(),
        additional,
        Composition {
            extensions: extension,
            ..Default::default()
        },
    )
    .unwrap();
    let generation = host.active().unwrap();
    let snapshot = host
        .plugins()
        .snapshots()
        .find(|snapshot| snapshot.get::<ModelProfile>("model").is_some())
        .unwrap();
    let expected = snapshot.get::<ModelProfile>("model").unwrap();
    assert_eq!(expected.provider, "fake-provider");
    assert_eq!(expected.model, "fake-capability-model");
    assert_eq!(expected.context_tokens, Some(32_768));
    assert!(expected.capabilities.contains("reasoning"));
    assert!(expected.capabilities.contains("structured-arguments"));

    let mut events = Vec::new();
    run_turn(&generation, "secret-test-payload".into(), 256, |event| {
        events.push(event.clone())
    })
    .unwrap();
    let profiles: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::ModelInfo(profile) => Some(profile),
            _ => None,
        })
        .collect();
    assert_eq!(profiles, vec![&*expected, &*expected]);
    assert!(events.contains(&AgentEvent::Delta("secret-test-payload".repeat(2))));
    assert_eq!(events.last(), Some(&AgentEvent::Completed));
    let reports: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::ProviderStatus(report) => Some(report),
            _ => None,
        })
        .collect();
    assert_eq!(reports.len(), 2);
    for (i, report) in reports.iter().enumerate() {
        assert_eq!(report.provider, expected.provider);
        assert_eq!(report.model, expected.model);
        assert_eq!(report.status.usage.as_ref().unwrap().input_tokens, Some(10));
        assert_eq!(report.status.windows[0].remaining, Some(59 - i as u64));
        assert_eq!(report.status.windows[0].resets_at, Some(1_800_000_060));
        assert_eq!(report.status.windows[1].remaining, None);
        assert_eq!(report.status.windows[1].resets_at, Some(1_800_604_800));
    }

    let trace = records.lock().unwrap().clone();
    if logging {
        assert_eq!(
            trace
                .iter()
                .filter(|r| matches!(r, TraceRecord::Model(p) if p == &*expected))
                .count(),
            2
        );
        assert!(trace.contains(&TraceRecord::Prompt {
            bytes: "secret-test-payload".len()
        }));
        assert!(trace.contains(&TraceRecord::Requested {
            name: "repeat".into(),
            environment: environment.into(),
        }));
        assert!(trace.contains(&TraceRecord::Dispatch {
            name: "repeat".into(),
            environment: environment.into(),
            argument_schema: FakeArguments::schema().name,
        }));
        assert!(trace.contains(&TraceRecord::Feedback {
            name: "repeat".into(),
            environment: environment.into(),
            succeeded: true,
            result_schema: String::schema().name,
        }));
        for report in reports {
            assert!(trace.contains(&TraceRecord::ProviderStatus(report.clone())));
        }
        assert!(trace.contains(&TraceRecord::Completed));
        assert!(!format!("{trace:?}").contains("secret-test-payload"));
    } else {
        assert!(trace.is_empty());
    }
    (events, trace)
}

#[test]
fn selected_capabilities_and_structured_tool_feedback_are_visible_in_both_profiles() {
    for advanced in [false, true] {
        let (normal, _) = scenario(advanced, false);
        let (development, trace) = scenario(advanced, true);
        assert_eq!(normal, development);
        assert!(!trace.is_empty());
    }
}

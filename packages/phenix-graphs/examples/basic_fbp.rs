//! Three local closure components, including a genuinely zero-output sink.
use phenix_graph::{component::ComponentDef, network::NetworkBuilder, runtime::SessionState};
use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};

fn main() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let collected = seen.clone();
    let mut b = NetworkBuilder::new();
    let source = b
        .add("source", Arc::new(ComponentDef::source(|| 1..=3_i64)))
        .unwrap();
    b.start(&source).unwrap();
    let map = b
        .add(
            "double",
            Arc::new(ComponentDef::map(|n: i64| format!("value: {}", n * 2))),
        )
        .unwrap();
    let sink = b
        .add(
            "sink",
            Arc::new(ComponentDef::sink(move |text: &String| {
                println!("{text}");
                collected.lock().unwrap().push(text.clone());
            })),
        )
        .unwrap();
    let capacity = NonZeroUsize::new(1).unwrap();
    b.connect(
        source.output::<i64>("output").unwrap(),
        map.input::<i64>("input").unwrap(),
        capacity,
    )
    .unwrap();
    b.connect(
        map.output::<String>("output").unwrap(),
        sink.input::<String>("input").unwrap(),
        capacity,
    )
    .unwrap();
    let mut session = b.build().unwrap().compile().unwrap().start();
    assert_eq!(
        session.run_until_idle(64).state,
        SessionState::Completed
    );
    assert_eq!(*seen.lock().unwrap(), ["value: 2", "value: 4", "value: 6"]);
}

#[test]
fn closure_pipeline_terminates() {
    main();
}

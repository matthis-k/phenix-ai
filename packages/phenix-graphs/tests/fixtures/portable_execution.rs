use phenix_graph::{
    AllSameCohort, ContractValue, Deployment, GraphBuilder, InputSlot, InvocationCtx, NodeTemplate,
    OutputSlot, Port, ReportRetention, Store, Value, emit,
};
use portable_consumer::Number as ConsumerNumber;
use portable_provider::Number as ProviderNumber;

fn relay<T: ContractValue>(id: u16) -> NodeTemplate {
    NodeTemplate::with_action(
        id,
        vec![
            Port::new(
                0,
                vec![InputSlot::deferred::<T>(0).named("value")],
                AllSameCohort,
            )
            .named("run")
            .args::<T, _>(|values| values.get("value")),
        ],
        vec![OutputSlot::typed::<T>(0).named("value")],
        |mut ctx: InvocationCtx| Ok(vec![emit(0, ctx.take_prepared::<T>()?)]),
    )
}

fn main() {
    assert_ne!(
        std::any::TypeId::of::<ProviderNumber>(),
        std::any::TypeId::of::<ConsumerNumber>()
    );
    assert_ne!(ProviderNumber::SCHEMA, ConsumerNumber::SCHEMA);
    assert_eq!(
        ProviderNumber::schema_descriptor(),
        ConsumerNumber::schema_descriptor()
    );
    let mut graph = GraphBuilder::new("independent-crates");
    graph.add("provider", relay::<ProviderNumber>(1)).unwrap();
    graph.add("consumer", relay::<ConsumerNumber>(2)).unwrap();
    let source = graph
        .wire_output::<ProviderNumber>("provider", "value")
        .unwrap();
    let sink = graph
        .wire_input::<ConsumerNumber>("consumer", "run", "value")
        .unwrap();
    graph.connect_wire(source, sink).unwrap();
    let request = graph.expose_input("request", "provider").unwrap();
    graph.expose_output("result", "consumer").unwrap();
    graph.trigger("run", [request]).unwrap();
    let engine = Deployment::new(graph.compile().unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap();
    for value in [i64::MIN, -1, 0, 1, 42, i64::MAX] {
        let report = engine
            .fire("run", vec![Value::of(ProviderNumber { value })])
            .unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        assert_eq!(report.invocation_count(1), 1);
        assert_eq!(report.invocation_count(2), 1);
        let publications: Vec<_> = report.publications().collect();
        assert_eq!(publications.len(), 2);
        assert_eq!(
            publications[1].value::<ConsumerNumber>().unwrap().number,
            value
        );
    }
}

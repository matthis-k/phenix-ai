use super::*;

#[test]
fn repeated_unloads_release_graph_metadata_actions_values_and_reports() {
    for revision in 0..128 {
        let schema = Schema {
            name: format!("reload/{revision}"),
            revision,
            structure: "i64".into(),
            codec: "i64-le-v1".into(),
        }
        .identity();
        let weak = Arc::downgrade(&schema);
        let report = {
            let emitted_schema = schema.clone();
            let template = NodeTemplate::with_action(
                1,
                vec![
                    Port::new(
                        0,
                        vec![
                            InputSlot::new(0, schema.clone(), BufferMode::Deferred).named("value"),
                        ],
                        AllSameCohort,
                    )
                    .named("run"),
                ],
                vec![OutputSlot::new(0, schema.clone()).named("value")],
                move |_: InvocationCtx| {
                    Ok(vec![Emission {
                        slot: SlotId(0),
                        value: Value::from_wire_parts(emitted_schema.clone(), 42i64.encode()),
                        selection: None,
                    }])
                },
            );
            let cloned_template = template.clone();
            assert!(Arc::ptr_eq(
                &template.outputs[0].schema,
                &cloned_template.outputs[0].schema
            ));
            let mut builder = GraphBuilder::new("reload");
            builder.add("worker", template).unwrap();
            let input = builder.expose_input("request", "worker").unwrap();
            builder.trigger("run", [input.clone()]).unwrap();
            builder.expose_output("result", "worker").unwrap();
            let snapshot = builder.inspect("worker").unwrap().inspect().unwrap();
            let descriptor = snapshot.shape.inputs().next().unwrap().2;
            assert!(Arc::ptr_eq(&schema, &descriptor.schema));
            let builder_clone = builder.clone();
            let compiled = builder.compile().unwrap();
            let other_compiled = builder_clone.compile().unwrap();
            let compiled_snapshot = compiled.find("worker").unwrap().inspect().unwrap();
            assert_eq!(snapshot.shape, compiled_snapshot.shape);
            let graph_clone = compiled.clone();
            let engine = Deployment::new(compiled)
                .store(Store::new())
                .report_retention(ReportRetention::RetainPayloads)
                .build()
                .unwrap();
            let value = Value::from_wire_parts(schema, 42i64.encode());
            let cloned_value = value.clone();
            assert!(Arc::ptr_eq(value.schema(), cloned_value.schema()));
            let report = engine.fire("run", vec![value]).unwrap();
            assert!(report.success(), "{:?}", report.error_messages());
            assert_eq!(report.publications().count(), 1);
            assert!(weak.upgrade().is_some());
            drop((
                graph_clone,
                other_compiled,
                cloned_template,
                compiled_snapshot,
                snapshot,
                descriptor,
                input,
                cloned_value,
            ));
            report
        };
        // A retained report is a legitimate owner after the deployment unloads.
        assert!(weak.upgrade().is_some());
        drop(report);
        assert!(
            weak.upgrade().is_none(),
            "reload {revision} retained its schema"
        );
    }
}

#[test]
fn interface_errors_own_reclaimable_complete_wire_identities() {
    let base = i64::schema();
    for changed in [
        Schema {
            name: "other".into(),
            ..base.clone()
        },
        Schema {
            revision: 2,
            ..base.clone()
        },
        Schema {
            structure: "u64".into(),
            ..base.clone()
        },
        Schema {
            codec: "i64-be-v1".into(),
            ..base.clone()
        },
    ] {
        let expected = base.identity();
        let actual = changed.identity();
        let weak_expected = Arc::downgrade(&expected);
        let weak_actual = Arc::downgrade(&actual);
        let error = {
            let expected_shape = InterfaceShape::new()
                .output(
                    "value",
                    DeclaredValue {
                        schema: expected,
                        local_type: None,
                    },
                )
                .unwrap();
            let actual_shape = InterfaceShape::new()
                .output(
                    "value",
                    DeclaredValue {
                        schema: actual,
                        local_type: None,
                    },
                )
                .unwrap();
            expected_shape.check_shape(&actual_shape).unwrap_err()
        };
        let clone = error.clone();
        let (Some(TypeIdentity::Wire(expected)), Some(TypeIdentity::Wire(cloned))) =
            (&error.expected, &clone.expected)
        else {
            panic!("missing wire identities")
        };
        assert!(Arc::ptr_eq(expected, cloned));
        drop(error);
        assert!(weak_expected.upgrade().is_some());
        assert!(weak_actual.upgrade().is_some());
        drop(clone);
        assert!(weak_expected.upgrade().is_none());
        assert!(weak_actual.upgrade().is_none());
    }
}

#[cfg(feature = "derive")]
#[derive(ContractValue)]
#[contract(schema = "ownership/record")]
struct Record {
    value: i64,
}

#[cfg(feature = "derive")]
#[derive(ContractValue)]
#[contract(schema = "ownership/record")]
struct Independent {
    value: i64,
}

#[test]
#[cfg(feature = "derive")]
fn derived_identities_are_not_cached_and_local_type_ids_remain_distinct() {
    for _ in 0..128 {
        let value = Value::of(Record { value: 42 });
        let weak = Arc::downgrade(value.schema());
        let independent = Independent::wire_schema();
        assert_eq!(*value.schema(), independent);
        assert!(!Arc::ptr_eq(value.schema(), &independent));
        assert_eq!(value.get::<Independent>().unwrap().value, 42);
        assert_ne!(
            Value::local(Record { value: 42 }).identity(),
            value.identity()
        );
        assert_ne!(
            Value::local(Record { value: 42 }).identity(),
            Value::local(Independent { value: 42 }).identity()
        );
        drop(value);
        assert!(weak.upgrade().is_none());
    }
}

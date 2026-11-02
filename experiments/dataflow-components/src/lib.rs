//! Only resource extraction and offline graph algorithms: no ECS schedule.
#[cfg(test)]
mod tests {
    use bevy_ecs::{prelude::*, system::SystemState};
    #[derive(Resource)]
    struct Read(u64);
    #[derive(Resource)]
    struct Write(u64);
    #[test]
    fn signature_injection_without_scheduling_or_change_activation() {
        let mut world = World::new();
        world.insert_resource(Read(7));
        world.insert_resource(Write(0));
        let mut params: SystemState<(Res<Read>, ResMut<Write>)> = SystemState::new(&mut world);
        {
            let (r, mut w) = params.get_mut(&mut world).unwrap();
            w.0 += r.0;
        }
        params.apply(&mut world);
        assert_eq!(world.resource::<Write>().0, 7);
        world.resource_mut::<Read>().0 = 9;
        assert_eq!(
            world.resource::<Write>().0,
            7,
            "access/change does not execute a node"
        );
    }
    #[test]
    #[should_panic]
    fn conflicting_access_is_rejected() {
        let mut world = World::new();
        world.insert_resource(Write(0));
        let _: SystemState<(Res<Write>, ResMut<Write>)> = SystemState::new(&mut world);
    }
    #[test]
    fn topology_analysis_has_no_placement_policy() {
        use petgraph::{algo::toposort, stable_graph::StableDiGraph};
        let mut g = StableDiGraph::<&str, ()>::new();
        let t = g.add_node("Trigger");
        let p = g.add_node("Prepare");
        let m = g.add_node("Model");
        let tool = g.add_node("Tool");
        let a = g.add_node("Aggregate");
        for (f, t) in [(t, p), (p, m), (p, tool), (m, a), (tool, a)] {
            g.add_edge(f, t, ());
        }
        let order = toposort(&g, None).unwrap();
        assert_eq!(order[0], t);
        assert_eq!(*order.last().unwrap(), a);
        g.add_edge(a, t, ());
        assert!(toposort(&g, None).is_err());
    }
}

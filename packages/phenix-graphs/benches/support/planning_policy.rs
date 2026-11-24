pub fn configure(composer: &mut phenix_graph::compose::Composer) {
    composer.set_planning_limits(phenix_graph::compose::PlanningLimits {
        max_units: 50_000_000,
    });
}

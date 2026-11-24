pub fn configure(options: &mut phenix_kernel::BuildOptions) {
    options.planning_limits = phenix_graph::compose::PlanningLimits {
        max_units: 50_000_000,
    };
}

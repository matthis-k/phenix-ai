use phenix_core::{Authority, PermissionId};

pub fn default_application_root_authority() -> Authority {
    Authority::new([
        PermissionId::parse("kernel.persistence.schema").expect("static capability"),
        PermissionId::parse("kernel.persistence.read").expect("static capability"),
        PermissionId::parse("kernel.persistence.write").expect("static capability"),
        PermissionId::parse("network.http").expect("static capability"),
        PermissionId::parse("secrets.manage").expect("static capability"),
        PermissionId::parse("workspace.read").expect("static capability"),
        PermissionId::parse("workspace.write").expect("static capability"),
        PermissionId::parse("workspace.shell").expect("static capability"),
        PermissionId::parse("workspace.git").expect("static capability"),
    ])
}

pub fn runtime_orchestration_authority() -> Authority {
    Authority::new([
        PermissionId::parse("application.session.control").expect("static capability"),
        PermissionId::parse("runtime.generation.select").expect("static capability"),
        PermissionId::parse("runtime.plugin.inspect").expect("static capability"),
        PermissionId::parse("runtime.plugin.build").expect("static capability"),
        PermissionId::parse("runtime.plugin.trial").expect("static capability"),
        PermissionId::parse("runtime.plugin.promote").expect("static capability"),
        PermissionId::parse("runtime.plugin.retire").expect("static capability"),
    ])
}

pub fn default_suite_authority() -> Authority {
    let application = default_application_root_authority();
    let orchestration = runtime_orchestration_authority();
    Authority::new(
        application
            .permissions()
            .cloned()
            .chain(orchestration.permissions().cloned()),
    )
}

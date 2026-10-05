use phenix_sdk::{
    Authority, Call, PermissionId, Required, StaticComponentBehavior, StaticComponentImports,
};

#[phenix_sdk::interface("fixture.layer.authority@1")]
struct Models;

#[phenix_sdk::component]
struct Api;

#[phenix_sdk::component]
impl Api {
    #[phenix(
        export(Models),
        terminal,
        priority = 29,
        authority = Authority::new([PermissionId::parse("models.serve").unwrap()])
    )]
    fn run(&self, request: String) -> String {
        request
    }

    #[phenix(layer(
        Models,
        priority = 17,
        authority = Authority::new([PermissionId::parse("models.invoke").unwrap()])
    ))]
    fn policy(&self) {}

    #[phenix(
        listen("fixture.models.observed"),
        authority = Authority::new([PermissionId::parse("models.observe").unwrap()])
    )]
    fn observed(&self, _context: &phenix_sdk::EventContext, _event: String) {}
}

#[allow(dead_code)]
#[phenix_sdk::component]
struct Consumer {
    #[phenix(
        import,
        authority = Authority::new([PermissionId::parse("models.read").unwrap()])
    )]
    models: Required<Call<Models, String, String>>,
}

#[test]
fn component_authority_survives_macro_lowering() {
    let layer_authority = Authority::new([PermissionId::parse("models.invoke").unwrap()]);
    let export_authority = Authority::new([PermissionId::parse("models.serve").unwrap()]);
    let import_authority = Authority::new([PermissionId::parse("models.read").unwrap()]);
    let listener_authority = Authority::new([PermissionId::parse("models.observe").unwrap()]);

    let layers = <Api as StaticComponentBehavior>::layers();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].priority, 17);
    assert_eq!(layers[0].required_authority, layer_authority);

    let exports = <Api as StaticComponentBehavior>::exports();
    assert_eq!(exports.len(), 1);
    assert!(exports[0].terminal);
    assert_eq!(exports[0].priority, 29);
    assert_eq!(exports[0].required_authority, export_authority);

    let imports = <Consumer as StaticComponentImports>::imports();
    assert_eq!(imports.len(), 1);
    assert!(imports[0].required);
    assert_eq!(imports[0].authority, import_authority);

    let listeners = <Api as StaticComponentBehavior>::listeners();
    assert_eq!(listeners.len(), 1);
    assert_eq!(listeners[0].required_authority, listener_authority);

    let _listener: fn(&Api, &phenix_sdk::EventContext, String) = Api::observed;
    let api = Api;
    assert_eq!(api.run("request".into()), "request");
    api.policy();
}

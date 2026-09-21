use super::*;

record!(CapabilityList, "phenix.application.type.capability-list@1", {
    interface: ContractId,
    capabilities: Vec<ContractId>,
});
record!(AuthenticationMethod, "phenix.application.type.authentication-method@1", {
    id: String,
    name: String,
    description: Option<String>,
});
record!(AuthenticationMethods, "phenix.application.type.authentication-methods@1", {
    methods: Vec<AuthenticationMethod>,
});
record!(AuthenticateInput, "phenix.application.type.authenticate-input@1", { method_id: String });
variants!(AuthenticationResult, "phenix.application.type.authentication-result@1", {
    Authenticated,
    External { uri: String, instructions: Option<String> },
});
variants!(SelectionPresentation, "phenix.application.type.selection-presentation@1", {
    Model,
    Router,
});
record!(SelectionInfo, "phenix.application.type.selection-info@1", {
    id: RoutingProfileId,
    provider: PluginId,
    name: String,
    description: Option<String>,
    presentation: SelectionPresentation,
});
record!(Selections, "phenix.application.type.selections@1", {
    available: Vec<SelectionInfo>,
    selected: Option<RoutingProfileId>,
});
record!(SelectionSelectInput, "phenix.application.type.selection-select-input@1", {
    session_id: SessionId,
    selection_id: RoutingProfileId,
});
record!(SkillInfo, "phenix.application.type.skill-info@1", {
    id: SkillId,
    name: String,
    description: String,
    active: bool,
});
record!(Skills, "phenix.application.type.skills@1", { skills: Vec<SkillInfo> });
record!(SkillActivateInput, "phenix.application.type.skill-activate-input@1", {
    session_id: SessionId,
    skill_id: SkillId,
});
record!(CallableInfo, "phenix.application.type.callable-info@1", {
    id: CallableId,
    description: String,
    input: PhenixSchema,
    output: PhenixSchema,
});
record!(Callables, "phenix.application.type.callables@1", { callables: Vec<CallableInfo> });
record!(CallableInvokeInput, "phenix.application.type.callable-invoke-input@1", {
    session_id: SessionId,
    callable_id: CallableId,
    input: PhenixValue,
});
record!(CallableResult, "phenix.application.type.callable-result@1", { output: PhenixValue });
record!(CapabilityInvokeInput, "phenix.application.type.capability-invoke-input@1", {
    callable: PhenixValue,
    input: PhenixValue,
});
record!(CapabilityInvokeResult, "phenix.application.type.capability-invoke-result@1", {
    output: PhenixValue,
});
// Admission keeps client-local behavior outside the durable runtime model. The
// value is checked as a live callable against `input` and `output` when the
// application accepts it because the fixed descriptor cannot express schemas
// that depend on sibling fields.
record!(ClientToolDefinition, "phenix.application.type.client-tool-definition@1", {
    id: CallableId,
    description: String,
    input: PhenixSchema,
    output: PhenixSchema,
    capabilities: Vec<String>,
    requires_permission: bool,
    invoke: PhenixValue,
});
record!(ClientToolAddInput, "phenix.application.type.client-tool-add-input@1", {
    session_id: SessionId,
    tool: ClientToolDefinition,
});
record!(ClientToolAdmission, "phenix.application.type.client-tool-admission@1", {
    admission_id: String,
    callable_id: CallableId,
});
record!(ClientToolRemoveInput, "phenix.application.type.client-tool-remove-input@1", {
    session_id: SessionId,
    admission_id: String,
});
// The complete language-facing SDK. `schema` is authoritative for callable
// leaves because a raw `CallableRef` does not retain input/output types.
record!(SdkValue, "phenix.application.type.sdk-value@1", {
    schema: PhenixSchema,
    value: PhenixValue,
});
variants!(Severity, "phenix.application.type.severity@1", { Info, Warning, Error });
record!(Diagnostic, "phenix.application.type.diagnostic@1", {
    code: String,
    severity: Severity,
    message: String,
    resource: Option<String>,
});
record!(Diagnostics, "phenix.application.type.diagnostics@1", { diagnostics: Vec<Diagnostic> });

// Generated from the fixed Phenix application descriptor. Do not edit.
pub const INTERFACE_ID: &str = "phenix.application@1";
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural0 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural1 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural2 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural3 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural4 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural5 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural6 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural7 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural8 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural9 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural10 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural11 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural12 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural13 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationError1Type { r#Cancelled,r#Closed,r#Conflict(Structural0),r#Disconnected,r#Failed(Structural1),r#InvalidInput(Structural2),r#InvalidPath(Structural3),r#InvalidResponse(Structural4),r#NotFound(Structural5),r#PermissionDenied(Structural6),r#SchemaMismatch(Structural7),r#StaleReference(Structural8),r#SubscriptionCapacity,r#TransactionConflict(Structural9),r#Unauthenticated(Structural10),r#UnknownValue(Structural11),r#UnsupportedCapability(Structural12),r#UnsupportedSnapshotPolicy(Structural13), }
impl phenix_core::PhenixContract for PhenixApplicationError1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.error@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeAcknowledged1Type {  }
impl phenix_core::PhenixContract for PhenixApplicationTypeAcknowledged1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.acknowledged@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeAuthenticateInput1Type { pub r#method_id: String,pub r#secret: Option<String>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeAuthenticateInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.authenticate-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeAuthenticationMethod1Type { pub r#authenticated: bool,pub r#description: Option<String>,pub r#id: String,pub r#kind: String,pub r#name: String,pub r#provider: String,pub r#provider_name: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeAuthenticationMethod1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.authentication-method@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural14 { pub r#authenticated: bool,pub r#description: Option<String>,pub r#id: String,pub r#kind: String,pub r#name: String,pub r#provider: String,pub r#provider_name: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeAuthenticationMethods1Type { pub r#methods: Vec<Structural14>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeAuthenticationMethods1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.authentication-methods@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural15 { pub r#instructions: Option<String>,pub r#uri: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeAuthenticationResult1Type { r#Authenticated,r#External(Structural15), }
impl phenix_core::PhenixContract for PhenixApplicationTypeAuthenticationResult1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.authentication-result@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeCallableInfo1Type { pub r#description: String,pub r#id: String,pub r#input: phenix_core::PhenixValue,pub r#output: phenix_core::PhenixValue, }
impl phenix_core::PhenixContract for PhenixApplicationTypeCallableInfo1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.callable-info@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeCallableInvocationResult1Type { pub r#output: phenix_core::PhenixValue, }
impl phenix_core::PhenixContract for PhenixApplicationTypeCallableInvocationResult1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.callable-invocation-result@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeCallableInvocation1Type { pub r#callable: phenix_core::PhenixValue,pub r#input: phenix_core::PhenixValue, }
impl phenix_core::PhenixContract for PhenixApplicationTypeCallableInvocation1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.callable-invocation@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeCallableInvokeInput1Type { pub r#callable_id: String,pub r#input: phenix_core::PhenixValue,pub r#session_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeCallableInvokeInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.callable-invoke-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeCallableResult1Type { pub r#output: phenix_core::PhenixValue, }
impl phenix_core::PhenixContract for PhenixApplicationTypeCallableResult1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.callable-result@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural16 { pub r#description: String,pub r#id: String,pub r#input: phenix_core::PhenixValue,pub r#output: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeCallables1Type { pub r#callables: Vec<Structural16>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeCallables1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.callables@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeCapabilityList1Type { pub r#capabilities: Vec<String>,pub r#interface: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeCapabilityList1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.capability-list@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural17 { pub r#capabilities: Vec<String>,pub r#description: String,pub r#id: String,pub r#input: phenix_core::PhenixValue,pub r#invoke: phenix_core::PhenixValue,pub r#output: phenix_core::PhenixValue,pub r#requires_permission: bool, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeClientToolAddInput1Type { pub r#session_id: String,pub r#tool: Structural17, }
impl phenix_core::PhenixContract for PhenixApplicationTypeClientToolAddInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.client-tool-add-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeClientToolAdmission1Type { pub r#admission_id: String,pub r#callable_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeClientToolAdmission1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.client-tool-admission@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeClientToolDefinition1Type { pub r#capabilities: Vec<String>,pub r#description: String,pub r#id: String,pub r#input: phenix_core::PhenixValue,pub r#invoke: phenix_core::PhenixValue,pub r#output: phenix_core::PhenixValue,pub r#requires_permission: bool, }
impl phenix_core::PhenixContract for PhenixApplicationTypeClientToolDefinition1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.client-tool-definition@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeClientToolRemoveInput1Type { pub r#admission_id: String,pub r#session_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeClientToolRemoveInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.client-tool-remove-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural18 { pub r#data: phenix_core::Bytes,pub r#mime_type: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural19 { pub r#mime_type: Option<String>,pub r#text: Option<String>,pub r#uri: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural20 { pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeContent1Type { r#Image(Structural18),r#Resource(Structural19),r#Text(Structural20), }
impl phenix_core::PhenixContract for PhenixApplicationTypeContent1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.content@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural21 { r#Error,r#Info,r#Warning, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeDiagnostic1Type { pub r#code: String,pub r#message: String,pub r#resource: Option<String>,pub r#severity: Structural21, }
impl phenix_core::PhenixContract for PhenixApplicationTypeDiagnostic1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.diagnostic@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural23 { r#Error,r#Info,r#Warning, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural22 { pub r#code: String,pub r#message: String,pub r#resource: Option<String>,pub r#severity: Structural23, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeDiagnostics1Type { pub r#diagnostics: Vec<Structural22>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeDiagnostics1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.diagnostics@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeElicitationRequest1Type { pub r#message: String,pub r#schema: phenix_core::PhenixValue,pub r#session_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeElicitationRequest1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.elicitation-request@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural24 { pub r#value: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeElicitationResponse1Type { r#Accepted(Structural24),r#Cancelled,r#Declined, }
impl phenix_core::PhenixContract for PhenixApplicationTypeElicitationResponse1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.elicitation-response@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeEmpty1Type {  }
impl phenix_core::PhenixContract for PhenixApplicationTypeEmpty1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.empty@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural25 { pub r#fraction: Option<f64>,pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural30 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural31 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural32 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural33 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural34 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural35 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural36 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural37 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural38 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural39 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural40 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural41 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural42 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural43 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural29 { r#Cancelled,r#Closed,r#Conflict(Structural30),r#Disconnected,r#Failed(Structural31),r#InvalidInput(Structural32),r#InvalidPath(Structural33),r#InvalidResponse(Structural34),r#NotFound(Structural35),r#PermissionDenied(Structural36),r#SchemaMismatch(Structural37),r#StaleReference(Structural38),r#SubscriptionCapacity,r#TransactionConflict(Structural39),r#Unauthenticated(Structural40),r#UnknownValue(Structural41),r#UnsupportedCapability(Structural42),r#UnsupportedSnapshotPolicy(Structural43), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural28 { pub r#error: Structural29, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural27 { r#Cancelled,r#Completed,r#Failed(Structural28),r#Pending,r#Running, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural26 { pub r#state: Structural27, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural44 { pub r#call_id: String,pub r#callable_id: String,pub r#input: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural47 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural48 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural49 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural50 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural51 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural52 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural53 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural54 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural55 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural56 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural57 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural58 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural59 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural60 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural46 { r#Cancelled,r#Closed,r#Conflict(Structural47),r#Disconnected,r#Failed(Structural48),r#InvalidInput(Structural49),r#InvalidPath(Structural50),r#InvalidResponse(Structural51),r#NotFound(Structural52),r#PermissionDenied(Structural53),r#SchemaMismatch(Structural54),r#StaleReference(Structural55),r#SubscriptionCapacity,r#TransactionConflict(Structural56),r#Unauthenticated(Structural57),r#UnknownValue(Structural58),r#UnsupportedCapability(Structural59),r#UnsupportedSnapshotPolicy(Structural60), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural45 { pub r#call_id: String,pub r#error: Structural46, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural61 { pub r#call_id: String,pub r#output: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeExecutionChange1Type { r#Progress(Structural25),r#State(Structural26),r#ToolCall(Structural44),r#ToolFailed(Structural45),r#ToolResult(Structural61), }
impl phenix_core::PhenixContract for PhenixApplicationTypeExecutionChange1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.execution-change@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural65 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural66 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural67 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural68 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural69 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural70 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural71 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural72 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural73 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural74 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural75 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural76 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural77 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural78 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural64 { r#Cancelled,r#Closed,r#Conflict(Structural65),r#Disconnected,r#Failed(Structural66),r#InvalidInput(Structural67),r#InvalidPath(Structural68),r#InvalidResponse(Structural69),r#NotFound(Structural70),r#PermissionDenied(Structural71),r#SchemaMismatch(Structural72),r#StaleReference(Structural73),r#SubscriptionCapacity,r#TransactionConflict(Structural74),r#Unauthenticated(Structural75),r#UnknownValue(Structural76),r#UnsupportedCapability(Structural77),r#UnsupportedSnapshotPolicy(Structural78), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural63 { pub r#error: Structural64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural62 { r#Cancelled,r#Completed,r#Failed(Structural63),r#Pending,r#Running, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeExecutionInfo1Type { pub r#execution_id: String,pub r#parent: Option<String>,pub r#state: Structural62, }
impl phenix_core::PhenixContract for PhenixApplicationTypeExecutionInfo1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.execution-info@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeExecutionInput1Type { pub r#execution_id: String,pub r#session_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeExecutionInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.execution-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural81 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural82 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural83 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural84 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural85 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural86 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural87 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural88 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural89 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural90 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural91 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural92 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural93 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural94 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural80 { r#Cancelled,r#Closed,r#Conflict(Structural81),r#Disconnected,r#Failed(Structural82),r#InvalidInput(Structural83),r#InvalidPath(Structural84),r#InvalidResponse(Structural85),r#NotFound(Structural86),r#PermissionDenied(Structural87),r#SchemaMismatch(Structural88),r#StaleReference(Structural89),r#SubscriptionCapacity,r#TransactionConflict(Structural90),r#Unauthenticated(Structural91),r#UnknownValue(Structural92),r#UnsupportedCapability(Structural93),r#UnsupportedSnapshotPolicy(Structural94), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural79 { pub r#error: Structural80, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeExecutionState1Type { r#Cancelled,r#Completed,r#Failed(Structural79),r#Pending,r#Running, }
impl phenix_core::PhenixContract for PhenixApplicationTypeExecutionState1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.execution-state@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural99 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural100 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural101 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural102 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural103 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural104 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural105 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural106 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural107 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural108 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural109 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural110 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural111 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural112 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural98 { r#Cancelled,r#Closed,r#Conflict(Structural99),r#Disconnected,r#Failed(Structural100),r#InvalidInput(Structural101),r#InvalidPath(Structural102),r#InvalidResponse(Structural103),r#NotFound(Structural104),r#PermissionDenied(Structural105),r#SchemaMismatch(Structural106),r#StaleReference(Structural107),r#SubscriptionCapacity,r#TransactionConflict(Structural108),r#Unauthenticated(Structural109),r#UnknownValue(Structural110),r#UnsupportedCapability(Structural111),r#UnsupportedSnapshotPolicy(Structural112), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural97 { pub r#error: Structural98, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural96 { r#Cancelled,r#Completed,r#Failed(Structural97),r#Pending,r#Running, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural95 { pub r#execution_id: String,pub r#parent: Option<String>,pub r#state: Structural96, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeExecutionTree1Type { pub r#executions: Vec<Structural95>,pub r#session_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeExecutionTree1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.execution-tree@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural114 { pub r#fraction: Option<f64>,pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural119 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural120 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural121 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural122 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural123 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural124 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural125 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural126 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural127 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural128 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural129 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural130 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural131 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural132 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural118 { r#Cancelled,r#Closed,r#Conflict(Structural119),r#Disconnected,r#Failed(Structural120),r#InvalidInput(Structural121),r#InvalidPath(Structural122),r#InvalidResponse(Structural123),r#NotFound(Structural124),r#PermissionDenied(Structural125),r#SchemaMismatch(Structural126),r#StaleReference(Structural127),r#SubscriptionCapacity,r#TransactionConflict(Structural128),r#Unauthenticated(Structural129),r#UnknownValue(Structural130),r#UnsupportedCapability(Structural131),r#UnsupportedSnapshotPolicy(Structural132), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural117 { pub r#error: Structural118, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural116 { r#Cancelled,r#Completed,r#Failed(Structural117),r#Pending,r#Running, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural115 { pub r#state: Structural116, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural133 { pub r#call_id: String,pub r#callable_id: String,pub r#input: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural136 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural137 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural138 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural139 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural140 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural141 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural142 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural143 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural144 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural145 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural146 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural147 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural148 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural149 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural135 { r#Cancelled,r#Closed,r#Conflict(Structural136),r#Disconnected,r#Failed(Structural137),r#InvalidInput(Structural138),r#InvalidPath(Structural139),r#InvalidResponse(Structural140),r#NotFound(Structural141),r#PermissionDenied(Structural142),r#SchemaMismatch(Structural143),r#StaleReference(Structural144),r#SubscriptionCapacity,r#TransactionConflict(Structural145),r#Unauthenticated(Structural146),r#UnknownValue(Structural147),r#UnsupportedCapability(Structural148),r#UnsupportedSnapshotPolicy(Structural149), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural134 { pub r#call_id: String,pub r#error: Structural135, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural150 { pub r#call_id: String,pub r#output: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural113 { r#Progress(Structural114),r#State(Structural115),r#ToolCall(Structural133),r#ToolFailed(Structural134),r#ToolResult(Structural150), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeExecutionUpdate1Type { pub r#execution_id: String,pub r#sequence: u64,pub r#session_id: String,pub r#update: Structural113, }
impl phenix_core::PhenixContract for PhenixApplicationTypeExecutionUpdate1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.execution-update@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural151 { pub r#message: String,pub r#schema: phenix_core::PhenixValue,pub r#session_id: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural153 { pub r#value: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural152 { r#Accepted(Structural153),r#Cancelled,r#Declined, }
#[derive(Clone, Debug, PartialEq)]
pub struct Callable154(pub phenix_core::CallableRef);
impl phenix_core::ValueCodec for Callable154 { fn phenix_type() -> phenix_core::PhenixSchema { phenix_core::PhenixSchema::Callable { contract: phenix_core::ContractId::parse("phenix.application.elicitation@1").expect("generated callable contract is valid"), input: Box::new(<Structural151 as phenix_core::HasPhenixSchema>::phenix_schema()), output: Box::new(<Structural152 as phenix_core::HasPhenixSchema>::phenix_schema()) } } fn to_value(&self) -> phenix_core::PhenixValue { phenix_core::PhenixValue::Callable(self.0.clone()) } fn from_value(value: &phenix_core::PhenixValue) -> Result<Self, phenix_core::ValueError> { <Self as phenix_core::ValueCodec>::phenix_type().parse(value)?; match value { phenix_core::PhenixValue::Callable(reference) => Ok(Self(reference.clone())), _ => unreachable!("validated callable value"), } } }
impl From<&Callable154> for phenix_core::PhenixValue { fn from(value: &Callable154) -> Self { <Callable154 as phenix_core::ValueCodec>::to_value(value) } }
impl TryFrom<phenix_core::Exact<&phenix_core::PhenixValue>> for Callable154 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Exact<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::from_value(value.0) } }
impl TryFrom<phenix_core::Project<&phenix_core::PhenixValue>> for Callable154 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Project<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::project_from_value(value.0) } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural155 { pub r#call_id: String,pub r#description: String,pub r#execution_id: String,pub r#session_id: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural156 { r#AllowOnce,r#Cancelled,r#Deny, }
#[derive(Clone, Debug, PartialEq)]
pub struct Callable157(pub phenix_core::CallableRef);
impl phenix_core::ValueCodec for Callable157 { fn phenix_type() -> phenix_core::PhenixSchema { phenix_core::PhenixSchema::Callable { contract: phenix_core::ContractId::parse("phenix.application.permission@1").expect("generated callable contract is valid"), input: Box::new(<Structural155 as phenix_core::HasPhenixSchema>::phenix_schema()), output: Box::new(<Structural156 as phenix_core::HasPhenixSchema>::phenix_schema()) } } fn to_value(&self) -> phenix_core::PhenixValue { phenix_core::PhenixValue::Callable(self.0.clone()) } fn from_value(value: &phenix_core::PhenixValue) -> Result<Self, phenix_core::ValueError> { <Self as phenix_core::ValueCodec>::phenix_type().parse(value)?; match value { phenix_core::PhenixValue::Callable(reference) => Ok(Self(reference.clone())), _ => unreachable!("validated callable value"), } } }
impl From<&Callable157> for phenix_core::PhenixValue { fn from(value: &Callable157) -> Self { <Callable157 as phenix_core::ValueCodec>::to_value(value) } }
impl TryFrom<phenix_core::Exact<&phenix_core::PhenixValue>> for Callable157 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Exact<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::from_value(value.0) } }
impl TryFrom<phenix_core::Project<&phenix_core::PhenixValue>> for Callable157 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Project<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::project_from_value(value.0) } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeInteractionHandlers1Type { pub r#elicitation: Option<Callable154>,pub r#permission: Option<Callable157>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeInteractionHandlers1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.interaction-handlers@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural158 { pub r#cursor: String,pub r#kind: String,pub r#payload: phenix_core::PhenixValue,pub r#pid: u64,pub r#timestamp_ms: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeLogPage1Type { pub r#next_cursor: Option<String>,pub r#records: Vec<Structural158>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeLogPage1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.log-page@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeLogQueryInput1Type { pub r#cursor: Option<String>,pub r#execution_id: Option<String>,pub r#limit: Option<u64>,pub r#session_id: Option<String>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeLogQueryInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.log-query-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeLogRecord1Type { pub r#cursor: String,pub r#kind: String,pub r#payload: phenix_core::PhenixValue,pub r#pid: u64,pub r#timestamp_ms: u64, }
impl phenix_core::PhenixContract for PhenixApplicationTypeLogRecord1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.log-record@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural161 { pub r#path: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural162 { pub r#resource: String,pub r#service: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural160 { r#File(Structural161),r#Service(Structural162), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural159 { pub r#bytes: u64,pub r#digest: String,pub r#locator: Structural160,pub r#media_type: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeLogReferenceContent1Type { pub r#content: phenix_core::Bytes,pub r#reference: Structural159, }
impl phenix_core::PhenixContract for PhenixApplicationTypeLogReferenceContent1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.log-reference-content@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural165 { pub r#path: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural166 { pub r#resource: String,pub r#service: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural164 { r#File(Structural165),r#Service(Structural166), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural163 { pub r#bytes: u64,pub r#digest: String,pub r#locator: Structural164,pub r#media_type: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeLogReferenceInput1Type { pub r#reference: Structural163, }
impl phenix_core::PhenixContract for PhenixApplicationTypeLogReferenceInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.log-reference-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeMessageRole1Type { r#Assistant,r#User, }
impl phenix_core::PhenixContract for PhenixApplicationTypeMessageRole1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.message-role@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural168 { pub r#data: phenix_core::Bytes,pub r#mime_type: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural169 { pub r#mime_type: Option<String>,pub r#text: Option<String>,pub r#uri: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural170 { pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural167 { r#Image(Structural168),r#Resource(Structural169),r#Text(Structural170), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural171 { r#Assistant,r#User, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeMessage1Type { pub r#content: Vec<Structural167>,pub r#role: Structural171, }
impl phenix_core::PhenixContract for PhenixApplicationTypeMessage1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.message@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural174 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural175 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural176 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural173 { r#Field(Structural174),r#Index(Structural175),r#MapKey(Structural176),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural172 { pub r#segments: Vec<Structural173>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableAddress1Type { pub r#path: Structural172,pub r#value_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableAddress1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-address@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural181 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural182 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural183 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural180 { r#Field(Structural181),r#Index(Structural182),r#MapKey(Structural183),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural179 { pub r#segments: Vec<Structural180>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural178 { pub r#path: Structural179, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural177 { pub r#change: Structural178, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural188 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural189 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural190 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural187 { r#Field(Structural188),r#Index(Structural189),r#MapKey(Structural190),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural186 { pub r#segments: Vec<Structural187>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural185 { pub r#path: Structural186,pub r#value: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural184 { pub r#change: Structural185, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural195 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural196 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural197 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural194 { r#Field(Structural195),r#Index(Structural196),r#MapKey(Structural197),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural193 { pub r#segments: Vec<Structural194>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural192 { pub r#delete_count: u64,pub r#inserted: Vec<phenix_core::PhenixValue>,pub r#path: Structural193,pub r#start: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural191 { pub r#change: Structural192, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeObservableChange1Type { r#Remove(Structural177),r#Replace(Structural184),r#Splice(Structural191), }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableChange1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-change@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural201 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural202 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural203 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural200 { r#Field(Structural201),r#Index(Structural202),r#MapKey(Structural203),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural199 { pub r#segments: Vec<Structural200>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural198 { pub r#path: Structural199,pub r#value_id: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural211 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural212 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural213 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural210 { r#Field(Structural211),r#Index(Structural212),r#MapKey(Structural213),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural209 { pub r#segments: Vec<Structural210>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural208 { pub r#path: Structural209, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural207 { pub r#change: Structural208, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural218 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural219 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural220 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural217 { r#Field(Structural218),r#Index(Structural219),r#MapKey(Structural220),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural216 { pub r#segments: Vec<Structural217>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural215 { pub r#path: Structural216,pub r#value: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural214 { pub r#change: Structural215, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural225 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural226 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural227 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural224 { r#Field(Structural225),r#Index(Structural226),r#MapKey(Structural227),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural223 { pub r#segments: Vec<Structural224>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural222 { pub r#delete_count: u64,pub r#inserted: Vec<phenix_core::PhenixValue>,pub r#path: Structural223,pub r#start: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural221 { pub r#change: Structural222, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural206 { r#Remove(Structural207),r#Replace(Structural214),r#Splice(Structural221), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural205 { pub r#changes: Vec<Structural206>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural228 { pub r#value: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural204 { r#Diff(Structural205),r#Full(Structural228), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableDelivery1Type { pub r#address: Structural198,pub r#commit_id: Option<u64>,pub r#from_version: u64,pub r#generation: u64,pub r#payload: Structural204,pub r#subscription_id: u64,pub r#value_id: String,pub r#version: u64, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableDelivery1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-delivery@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural231 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural232 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural233 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural230 { r#Field(Structural231),r#Index(Structural232),r#MapKey(Structural233),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural229 { pub r#segments: Vec<Structural230>, }
#[derive(Clone, Debug, PartialEq)]
pub struct Object234(pub phenix_core::ObjectRef);
impl phenix_core::ValueCodec for Object234 { fn phenix_type() -> phenix_core::PhenixSchema { phenix_core::PhenixSchema::Object { contract: phenix_core::ContractId::parse("phenix.observable@1").expect("generated object contract is valid") } } fn to_value(&self) -> phenix_core::PhenixValue { phenix_core::PhenixValue::Object(self.0.clone()) } fn from_value(value: &phenix_core::PhenixValue) -> Result<Self, phenix_core::ValueError> { <Self as phenix_core::ValueCodec>::phenix_type().parse(value)?; match value { phenix_core::PhenixValue::Object(reference) => Ok(Self(reference.clone())), _ => unreachable!("validated object value"), } } }
impl From<&Object234> for phenix_core::PhenixValue { fn from(value: &Object234) -> Self { <Object234 as phenix_core::ValueCodec>::to_value(value) } }
impl TryFrom<phenix_core::Exact<&phenix_core::PhenixValue>> for Object234 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Exact<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::from_value(value.0) } }
impl TryFrom<phenix_core::Project<&phenix_core::PhenixValue>> for Object234 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Project<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::project_from_value(value.0) } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableGetInput1Type { pub r#path: Structural229,pub r#reference: Object234, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableGetInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-get-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeObservableInitial1Type { r#Full,r#None, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableInitial1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-initial@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural238 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural239 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural240 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural237 { r#Field(Structural238),r#Index(Structural239),r#MapKey(Structural240),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural236 { pub r#segments: Vec<Structural237>, }
#[derive(Clone, Debug, PartialEq)]
pub struct Object241(pub phenix_core::ObjectRef);
impl phenix_core::ValueCodec for Object241 { fn phenix_type() -> phenix_core::PhenixSchema { phenix_core::PhenixSchema::Object { contract: phenix_core::ContractId::parse("phenix.observable@1").expect("generated object contract is valid") } } fn to_value(&self) -> phenix_core::PhenixValue { phenix_core::PhenixValue::Object(self.0.clone()) } fn from_value(value: &phenix_core::PhenixValue) -> Result<Self, phenix_core::ValueError> { <Self as phenix_core::ValueCodec>::phenix_type().parse(value)?; match value { phenix_core::PhenixValue::Object(reference) => Ok(Self(reference.clone())), _ => unreachable!("validated object value"), } } }
impl From<&Object241> for phenix_core::PhenixValue { fn from(value: &Object241) -> Self { <Object241 as phenix_core::ValueCodec>::to_value(value) } }
impl TryFrom<phenix_core::Exact<&phenix_core::PhenixValue>> for Object241 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Exact<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::from_value(value.0) } }
impl TryFrom<phenix_core::Project<&phenix_core::PhenixValue>> for Object241 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Project<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::project_from_value(value.0) } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural242 { r#CopyOnChange,r#CurrentOnly, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural235 { pub r#binding_path: Vec<String>,pub r#namespace: String,pub r#path: Structural236,pub r#reference: Object241,pub r#resource: String,pub r#schema: phenix_core::PhenixValue,pub r#snapshot_policy: Structural242,pub r#value_id: String,pub r#version: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableList1Type { pub r#resources: Vec<Structural235>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableList1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-list@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeObservableMode1Type { r#Diff,r#Full, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableMode1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-mode@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural243 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural244 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural245 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeObservablePathSegment1Type { r#Field(Structural243),r#Index(Structural244),r#MapKey(Structural245),r#OptionPayload,r#VariantPayload, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservablePathSegment1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-path-segment@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural247 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural248 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural249 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural246 { r#Field(Structural247),r#Index(Structural248),r#MapKey(Structural249),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservablePath1Type { pub r#segments: Vec<Structural246>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservablePath1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-path@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural256 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural257 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural258 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural255 { r#Field(Structural256),r#Index(Structural257),r#MapKey(Structural258),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural254 { pub r#segments: Vec<Structural255>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural253 { pub r#path: Structural254, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural252 { pub r#change: Structural253, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural263 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural264 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural265 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural262 { r#Field(Structural263),r#Index(Structural264),r#MapKey(Structural265),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural261 { pub r#segments: Vec<Structural262>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural260 { pub r#path: Structural261,pub r#value: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural259 { pub r#change: Structural260, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural270 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural271 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural272 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural269 { r#Field(Structural270),r#Index(Structural271),r#MapKey(Structural272),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural268 { pub r#segments: Vec<Structural269>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural267 { pub r#delete_count: u64,pub r#inserted: Vec<phenix_core::PhenixValue>,pub r#path: Structural268,pub r#start: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural266 { pub r#change: Structural267, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural251 { r#Remove(Structural252),r#Replace(Structural259),r#Splice(Structural266), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural250 { pub r#changes: Vec<Structural251>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural273 { pub r#value: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeObservablePayload1Type { r#Diff(Structural250),r#Full(Structural273), }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservablePayload1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-payload@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural276 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural277 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural278 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural275 { r#Field(Structural276),r#Index(Structural277),r#MapKey(Structural278),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural274 { pub r#segments: Vec<Structural275>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableRemove1Type { pub r#path: Structural274, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableRemove1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-remove@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural281 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural282 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural283 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural280 { r#Field(Structural281),r#Index(Structural282),r#MapKey(Structural283),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural279 { pub r#segments: Vec<Structural280>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableReplace1Type { pub r#path: Structural279,pub r#value: phenix_core::PhenixValue, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableReplace1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-replace@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural286 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural287 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural288 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural285 { r#Field(Structural286),r#Index(Structural287),r#MapKey(Structural288),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural284 { pub r#segments: Vec<Structural285>, }
#[derive(Clone, Debug, PartialEq)]
pub struct Object289(pub phenix_core::ObjectRef);
impl phenix_core::ValueCodec for Object289 { fn phenix_type() -> phenix_core::PhenixSchema { phenix_core::PhenixSchema::Object { contract: phenix_core::ContractId::parse("phenix.observable@1").expect("generated object contract is valid") } } fn to_value(&self) -> phenix_core::PhenixValue { phenix_core::PhenixValue::Object(self.0.clone()) } fn from_value(value: &phenix_core::PhenixValue) -> Result<Self, phenix_core::ValueError> { <Self as phenix_core::ValueCodec>::phenix_type().parse(value)?; match value { phenix_core::PhenixValue::Object(reference) => Ok(Self(reference.clone())), _ => unreachable!("validated object value"), } } }
impl From<&Object289> for phenix_core::PhenixValue { fn from(value: &Object289) -> Self { <Object289 as phenix_core::ValueCodec>::to_value(value) } }
impl TryFrom<phenix_core::Exact<&phenix_core::PhenixValue>> for Object289 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Exact<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::from_value(value.0) } }
impl TryFrom<phenix_core::Project<&phenix_core::PhenixValue>> for Object289 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Project<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::project_from_value(value.0) } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural290 { r#CopyOnChange,r#CurrentOnly, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableResource1Type { pub r#binding_path: Vec<String>,pub r#namespace: String,pub r#path: Structural284,pub r#reference: Object289,pub r#resource: String,pub r#schema: phenix_core::PhenixValue,pub r#snapshot_policy: Structural290,pub r#value_id: String,pub r#version: u64, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableResource1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-resource@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeObservableScope1Type { r#Exact,r#Recursive, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableScope1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-scope@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeObservableSnapshotPolicy1Type { r#CopyOnChange,r#CurrentOnly, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableSnapshotPolicy1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-snapshot-policy@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural293 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural294 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural295 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural292 { r#Field(Structural293),r#Index(Structural294),r#MapKey(Structural295),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural291 { pub r#segments: Vec<Structural292>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableSplice1Type { pub r#delete_count: u64,pub r#inserted: Vec<phenix_core::PhenixValue>,pub r#path: Structural291,pub r#start: u64, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableSplice1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-splice@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural296 { r#Full,r#None, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural297 { r#Diff,r#Full, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural300 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural301 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural302 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural299 { r#Field(Structural300),r#Index(Structural301),r#MapKey(Structural302),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural298 { pub r#segments: Vec<Structural299>, }
#[derive(Clone, Debug, PartialEq)]
pub struct Object303(pub phenix_core::ObjectRef);
impl phenix_core::ValueCodec for Object303 { fn phenix_type() -> phenix_core::PhenixSchema { phenix_core::PhenixSchema::Object { contract: phenix_core::ContractId::parse("phenix.observable@1").expect("generated object contract is valid") } } fn to_value(&self) -> phenix_core::PhenixValue { phenix_core::PhenixValue::Object(self.0.clone()) } fn from_value(value: &phenix_core::PhenixValue) -> Result<Self, phenix_core::ValueError> { <Self as phenix_core::ValueCodec>::phenix_type().parse(value)?; match value { phenix_core::PhenixValue::Object(reference) => Ok(Self(reference.clone())), _ => unreachable!("validated object value"), } } }
impl From<&Object303> for phenix_core::PhenixValue { fn from(value: &Object303) -> Self { <Object303 as phenix_core::ValueCodec>::to_value(value) } }
impl TryFrom<phenix_core::Exact<&phenix_core::PhenixValue>> for Object303 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Exact<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::from_value(value.0) } }
impl TryFrom<phenix_core::Project<&phenix_core::PhenixValue>> for Object303 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Project<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::project_from_value(value.0) } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural304 { r#Exact,r#Recursive, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableSubscribeInput1Type { pub r#initial: Structural296,pub r#mode: Structural297,pub r#path: Structural298,pub r#reference: Object303,pub r#scope: Structural304, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableSubscribeInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-subscribe-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural309 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural310 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural311 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural308 { r#Field(Structural309),r#Index(Structural310),r#MapKey(Structural311),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural307 { pub r#segments: Vec<Structural308>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural306 { pub r#path: Structural307,pub r#value_id: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural319 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural320 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural321 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural318 { r#Field(Structural319),r#Index(Structural320),r#MapKey(Structural321),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural317 { pub r#segments: Vec<Structural318>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural316 { pub r#path: Structural317, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural315 { pub r#change: Structural316, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural326 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural327 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural328 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural325 { r#Field(Structural326),r#Index(Structural327),r#MapKey(Structural328),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural324 { pub r#segments: Vec<Structural325>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural323 { pub r#path: Structural324,pub r#value: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural322 { pub r#change: Structural323, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural333 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural334 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural335 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural332 { r#Field(Structural333),r#Index(Structural334),r#MapKey(Structural335),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural331 { pub r#segments: Vec<Structural332>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural330 { pub r#delete_count: u64,pub r#inserted: Vec<phenix_core::PhenixValue>,pub r#path: Structural331,pub r#start: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural329 { pub r#change: Structural330, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural314 { r#Remove(Structural315),r#Replace(Structural322),r#Splice(Structural329), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural313 { pub r#changes: Vec<Structural314>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural336 { pub r#value: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural312 { r#Diff(Structural313),r#Full(Structural336), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural305 { pub r#address: Structural306,pub r#commit_id: Option<u64>,pub r#from_version: u64,pub r#generation: u64,pub r#payload: Structural312,pub r#subscription_id: u64,pub r#value_id: String,pub r#version: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableSubscriptionResult1Type { pub r#generation: u64,pub r#initial: Option<Structural305>,pub r#subscription_id: u64, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableSubscriptionResult1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-subscription-result@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableUnsubscribeInput1Type { pub r#generation: u64,pub r#subscription_id: u64, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableUnsubscribeInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-unsubscribe-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableValue1Type { pub r#value: phenix_core::PhenixValue,pub r#value_id: String,pub r#version: u64, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableValue1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-value@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypePageInput1Type { pub r#cursor: Option<String>, }
impl phenix_core::PhenixContract for PhenixApplicationTypePageInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.page-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypePermissionRequest1Type { pub r#call_id: String,pub r#description: String,pub r#execution_id: String,pub r#session_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypePermissionRequest1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.permission-request@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypePermissionResponse1Type { r#AllowOnce,r#Cancelled,r#Deny, }
impl phenix_core::PhenixContract for PhenixApplicationTypePermissionResponse1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.permission-response@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural338 { pub r#data: phenix_core::Bytes,pub r#mime_type: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural339 { pub r#mime_type: Option<String>,pub r#text: Option<String>,pub r#uri: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural340 { pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural337 { r#Image(Structural338),r#Resource(Structural339),r#Text(Structural340), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypePromptInput1Type { pub r#content: Vec<Structural337>,pub r#session_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypePromptInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.prompt-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural341 { r#Cancelled,r#EndTurn,r#MaxTokens,r#Refused, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypePromptResult1Type { pub r#execution_id: String,pub r#stop_reason: Structural341, }
impl phenix_core::PhenixContract for PhenixApplicationTypePromptResult1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.prompt-result@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeProvenance1Type { pub r#execution_id: String,pub r#inputs: Vec<String>,pub r#model_id: Option<String>,pub r#outputs: Vec<String>,pub r#routing_profile: Option<String>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeProvenance1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.provenance@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural342 { r#Accept,r#Reject, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeReviewDecisionInput1Type { pub r#decision: Structural342,pub r#expected_revision: u64,pub r#review_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeReviewDecisionInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.review-decision-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeReviewDecision1Type { r#Accept,r#Reject, }
impl phenix_core::PhenixContract for PhenixApplicationTypeReviewDecision1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.review-decision@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural343 { pub r#id: String,pub r#new_count: u64,pub r#new_start: u64,pub r#old_count: u64,pub r#old_start: u64,pub r#unified_diff: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeReviewFile1Type { pub r#conflict: Option<String>,pub r#expected_version: String,pub r#hunks: Vec<Structural343>,pub r#uri: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeReviewFile1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.review-file@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeReviewHunk1Type { pub r#id: String,pub r#new_count: u64,pub r#new_start: u64,pub r#old_count: u64,pub r#old_start: u64,pub r#unified_diff: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeReviewHunk1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.review-hunk@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural345 { pub r#id: String,pub r#new_count: u64,pub r#new_start: u64,pub r#old_count: u64,pub r#old_start: u64,pub r#unified_diff: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural344 { pub r#conflict: Option<String>,pub r#expected_version: String,pub r#hunks: Vec<Structural345>,pub r#uri: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural347 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural346 { r#Accepted,r#Conflicted(Structural347),r#Pending,r#Rejected, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeReviewRecord1Type { pub r#execution_id: String,pub r#files: Vec<Structural344>,pub r#id: String,pub r#revision: u64,pub r#session_id: String,pub r#state: Structural346, }
impl phenix_core::PhenixContract for PhenixApplicationTypeReviewRecord1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.review-record@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural348 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeReviewState1Type { r#Accepted,r#Conflicted(Structural348),r#Pending,r#Rejected, }
impl phenix_core::PhenixContract for PhenixApplicationTypeReviewState1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.review-state@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSdkValue1Type { pub r#schema: phenix_core::PhenixValue,pub r#value: phenix_core::PhenixValue, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSdkValue1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.sdk-value@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSelectionDefaultSelectInput1Type { pub r#selection_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSelectionDefaultSelectInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.selection-default-select-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural349 { r#Model,r#Router, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSelectionInfo1Type { pub r#authenticated: bool,pub r#description: Option<String>,pub r#id: String,pub r#model: Option<String>,pub r#name: String,pub r#presentation: Structural349,pub r#provider: String,pub r#provider_name: String,pub r#thinking: Option<String>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSelectionInfo1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.selection-info@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeSelectionPresentation1Type { r#Model,r#Router, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSelectionPresentation1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.selection-presentation@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSelectionSelectInput1Type { pub r#selection_id: String,pub r#session_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSelectionSelectInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.selection-select-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural351 { r#Model,r#Router, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural350 { pub r#authenticated: bool,pub r#description: Option<String>,pub r#id: String,pub r#model: Option<String>,pub r#name: String,pub r#presentation: Structural351,pub r#provider: String,pub r#provider_name: String,pub r#thinking: Option<String>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSelections1Type { pub r#available: Vec<Structural350>,pub r#selected: Option<String>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSelections1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.selections@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural354 { r#Error,r#Info,r#Warning, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural353 { pub r#code: String,pub r#message: String,pub r#resource: Option<String>,pub r#severity: Structural354, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural352 { pub r#diagnostic: Structural353, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural357 { pub r#fraction: Option<f64>,pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural362 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural363 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural364 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural365 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural366 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural367 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural368 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural369 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural370 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural371 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural372 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural373 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural374 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural375 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural361 { r#Cancelled,r#Closed,r#Conflict(Structural362),r#Disconnected,r#Failed(Structural363),r#InvalidInput(Structural364),r#InvalidPath(Structural365),r#InvalidResponse(Structural366),r#NotFound(Structural367),r#PermissionDenied(Structural368),r#SchemaMismatch(Structural369),r#StaleReference(Structural370),r#SubscriptionCapacity,r#TransactionConflict(Structural371),r#Unauthenticated(Structural372),r#UnknownValue(Structural373),r#UnsupportedCapability(Structural374),r#UnsupportedSnapshotPolicy(Structural375), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural360 { pub r#error: Structural361, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural359 { r#Cancelled,r#Completed,r#Failed(Structural360),r#Pending,r#Running, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural358 { pub r#state: Structural359, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural376 { pub r#call_id: String,pub r#callable_id: String,pub r#input: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural379 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural380 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural381 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural382 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural383 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural384 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural385 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural386 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural387 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural388 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural389 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural390 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural391 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural392 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural378 { r#Cancelled,r#Closed,r#Conflict(Structural379),r#Disconnected,r#Failed(Structural380),r#InvalidInput(Structural381),r#InvalidPath(Structural382),r#InvalidResponse(Structural383),r#NotFound(Structural384),r#PermissionDenied(Structural385),r#SchemaMismatch(Structural386),r#StaleReference(Structural387),r#SubscriptionCapacity,r#TransactionConflict(Structural388),r#Unauthenticated(Structural389),r#UnknownValue(Structural390),r#UnsupportedCapability(Structural391),r#UnsupportedSnapshotPolicy(Structural392), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural377 { pub r#call_id: String,pub r#error: Structural378, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural393 { pub r#call_id: String,pub r#output: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural356 { r#Progress(Structural357),r#State(Structural358),r#ToolCall(Structural376),r#ToolFailed(Structural377),r#ToolResult(Structural393), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural355 { pub r#execution_id: String,pub r#update: Structural356, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural397 { pub r#data: phenix_core::Bytes,pub r#mime_type: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural398 { pub r#mime_type: Option<String>,pub r#text: Option<String>,pub r#uri: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural399 { pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural396 { r#Image(Structural397),r#Resource(Structural398),r#Text(Structural399), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural400 { r#Assistant,r#User, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural395 { pub r#content: Vec<Structural396>,pub r#role: Structural400, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural394 { pub r#message: Structural395, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural401 { pub r#title: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural405 { pub r#id: String,pub r#new_count: u64,pub r#new_start: u64,pub r#old_count: u64,pub r#old_start: u64,pub r#unified_diff: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural404 { pub r#conflict: Option<String>,pub r#expected_version: String,pub r#hunks: Vec<Structural405>,pub r#uri: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural407 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural406 { r#Accepted,r#Conflicted(Structural407),r#Pending,r#Rejected, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural403 { pub r#execution_id: String,pub r#files: Vec<Structural404>,pub r#id: String,pub r#revision: u64,pub r#session_id: String,pub r#state: Structural406, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural402 { pub r#review: Structural403, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural408 { pub r#execution_id: String,pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeSessionChange1Type { r#Closed,r#Diagnostic(Structural352),r#Execution(Structural355),r#Message(Structural394),r#Renamed(Structural401),r#Review(Structural402),r#TextDelta(Structural408), }
impl phenix_core::PhenixContract for PhenixApplicationTypeSessionChange1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.session-change@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSessionCreateInput1Type { pub r#title: Option<String>,pub r#working_directory: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSessionCreateInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.session-create-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSessionInfo1Type { pub r#session_id: String,pub r#title: Option<String>,pub r#working_directory: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSessionInfo1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.session-info@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSessionInput1Type { pub r#session_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSessionInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.session-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSessionLineage1Type { pub r#children: Vec<String>,pub r#parent: Option<String>,pub r#session_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSessionLineage1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.session-lineage@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural409 { pub r#session_id: String,pub r#title: Option<String>,pub r#working_directory: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSessionList1Type { pub r#next_cursor: Option<String>,pub r#sessions: Vec<Structural409>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSessionList1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.session-list@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural411 { pub r#session_id: String,pub r#title: Option<String>,pub r#working_directory: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural416 { r#Error,r#Info,r#Warning, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural415 { pub r#code: String,pub r#message: String,pub r#resource: Option<String>,pub r#severity: Structural416, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural414 { pub r#diagnostic: Structural415, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural419 { pub r#fraction: Option<f64>,pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural424 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural425 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural426 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural427 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural428 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural429 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural430 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural431 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural432 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural433 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural434 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural435 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural436 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural437 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural423 { r#Cancelled,r#Closed,r#Conflict(Structural424),r#Disconnected,r#Failed(Structural425),r#InvalidInput(Structural426),r#InvalidPath(Structural427),r#InvalidResponse(Structural428),r#NotFound(Structural429),r#PermissionDenied(Structural430),r#SchemaMismatch(Structural431),r#StaleReference(Structural432),r#SubscriptionCapacity,r#TransactionConflict(Structural433),r#Unauthenticated(Structural434),r#UnknownValue(Structural435),r#UnsupportedCapability(Structural436),r#UnsupportedSnapshotPolicy(Structural437), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural422 { pub r#error: Structural423, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural421 { r#Cancelled,r#Completed,r#Failed(Structural422),r#Pending,r#Running, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural420 { pub r#state: Structural421, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural438 { pub r#call_id: String,pub r#callable_id: String,pub r#input: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural441 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural442 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural443 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural444 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural445 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural446 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural447 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural448 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural449 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural450 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural451 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural452 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural453 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural454 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural440 { r#Cancelled,r#Closed,r#Conflict(Structural441),r#Disconnected,r#Failed(Structural442),r#InvalidInput(Structural443),r#InvalidPath(Structural444),r#InvalidResponse(Structural445),r#NotFound(Structural446),r#PermissionDenied(Structural447),r#SchemaMismatch(Structural448),r#StaleReference(Structural449),r#SubscriptionCapacity,r#TransactionConflict(Structural450),r#Unauthenticated(Structural451),r#UnknownValue(Structural452),r#UnsupportedCapability(Structural453),r#UnsupportedSnapshotPolicy(Structural454), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural439 { pub r#call_id: String,pub r#error: Structural440, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural455 { pub r#call_id: String,pub r#output: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural418 { r#Progress(Structural419),r#State(Structural420),r#ToolCall(Structural438),r#ToolFailed(Structural439),r#ToolResult(Structural455), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural417 { pub r#execution_id: String,pub r#update: Structural418, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural459 { pub r#data: phenix_core::Bytes,pub r#mime_type: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural460 { pub r#mime_type: Option<String>,pub r#text: Option<String>,pub r#uri: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural461 { pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural458 { r#Image(Structural459),r#Resource(Structural460),r#Text(Structural461), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural462 { r#Assistant,r#User, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural457 { pub r#content: Vec<Structural458>,pub r#role: Structural462, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural456 { pub r#message: Structural457, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural463 { pub r#title: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural467 { pub r#id: String,pub r#new_count: u64,pub r#new_start: u64,pub r#old_count: u64,pub r#old_start: u64,pub r#unified_diff: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural466 { pub r#conflict: Option<String>,pub r#expected_version: String,pub r#hunks: Vec<Structural467>,pub r#uri: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural469 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural468 { r#Accepted,r#Conflicted(Structural469),r#Pending,r#Rejected, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural465 { pub r#execution_id: String,pub r#files: Vec<Structural466>,pub r#id: String,pub r#revision: u64,pub r#session_id: String,pub r#state: Structural468, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural464 { pub r#review: Structural465, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural470 { pub r#execution_id: String,pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural413 { r#Closed,r#Diagnostic(Structural414),r#Execution(Structural417),r#Message(Structural456),r#Renamed(Structural463),r#Review(Structural464),r#TextDelta(Structural470), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural412 { pub r#sequence: u64,pub r#session_id: String,pub r#update: Structural413, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural410 { pub r#session: Structural411,pub r#through_sequence: u64,pub r#updates: Vec<Structural412>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSessionProjectionState1Type { pub r#sessions: std::collections::BTreeMap<String, Structural410>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSessionProjectionState1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.session-projection-state@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural471 { pub r#session_id: String,pub r#title: Option<String>,pub r#working_directory: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural476 { r#Error,r#Info,r#Warning, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural475 { pub r#code: String,pub r#message: String,pub r#resource: Option<String>,pub r#severity: Structural476, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural474 { pub r#diagnostic: Structural475, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural479 { pub r#fraction: Option<f64>,pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural484 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural485 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural486 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural487 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural488 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural489 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural490 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural491 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural492 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural493 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural494 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural495 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural496 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural497 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural483 { r#Cancelled,r#Closed,r#Conflict(Structural484),r#Disconnected,r#Failed(Structural485),r#InvalidInput(Structural486),r#InvalidPath(Structural487),r#InvalidResponse(Structural488),r#NotFound(Structural489),r#PermissionDenied(Structural490),r#SchemaMismatch(Structural491),r#StaleReference(Structural492),r#SubscriptionCapacity,r#TransactionConflict(Structural493),r#Unauthenticated(Structural494),r#UnknownValue(Structural495),r#UnsupportedCapability(Structural496),r#UnsupportedSnapshotPolicy(Structural497), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural482 { pub r#error: Structural483, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural481 { r#Cancelled,r#Completed,r#Failed(Structural482),r#Pending,r#Running, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural480 { pub r#state: Structural481, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural498 { pub r#call_id: String,pub r#callable_id: String,pub r#input: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural501 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural502 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural503 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural504 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural505 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural506 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural507 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural508 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural509 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural510 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural511 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural512 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural513 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural514 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural500 { r#Cancelled,r#Closed,r#Conflict(Structural501),r#Disconnected,r#Failed(Structural502),r#InvalidInput(Structural503),r#InvalidPath(Structural504),r#InvalidResponse(Structural505),r#NotFound(Structural506),r#PermissionDenied(Structural507),r#SchemaMismatch(Structural508),r#StaleReference(Structural509),r#SubscriptionCapacity,r#TransactionConflict(Structural510),r#Unauthenticated(Structural511),r#UnknownValue(Structural512),r#UnsupportedCapability(Structural513),r#UnsupportedSnapshotPolicy(Structural514), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural499 { pub r#call_id: String,pub r#error: Structural500, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural515 { pub r#call_id: String,pub r#output: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural478 { r#Progress(Structural479),r#State(Structural480),r#ToolCall(Structural498),r#ToolFailed(Structural499),r#ToolResult(Structural515), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural477 { pub r#execution_id: String,pub r#update: Structural478, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural519 { pub r#data: phenix_core::Bytes,pub r#mime_type: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural520 { pub r#mime_type: Option<String>,pub r#text: Option<String>,pub r#uri: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural521 { pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural518 { r#Image(Structural519),r#Resource(Structural520),r#Text(Structural521), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural522 { r#Assistant,r#User, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural517 { pub r#content: Vec<Structural518>,pub r#role: Structural522, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural516 { pub r#message: Structural517, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural523 { pub r#title: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural527 { pub r#id: String,pub r#new_count: u64,pub r#new_start: u64,pub r#old_count: u64,pub r#old_start: u64,pub r#unified_diff: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural526 { pub r#conflict: Option<String>,pub r#expected_version: String,pub r#hunks: Vec<Structural527>,pub r#uri: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural529 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural528 { r#Accepted,r#Conflicted(Structural529),r#Pending,r#Rejected, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural525 { pub r#execution_id: String,pub r#files: Vec<Structural526>,pub r#id: String,pub r#revision: u64,pub r#session_id: String,pub r#state: Structural528, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural524 { pub r#review: Structural525, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural530 { pub r#execution_id: String,pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural473 { r#Closed,r#Diagnostic(Structural474),r#Execution(Structural477),r#Message(Structural516),r#Renamed(Structural523),r#Review(Structural524),r#TextDelta(Structural530), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural472 { pub r#sequence: u64,pub r#session_id: String,pub r#update: Structural473, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSessionProjection1Type { pub r#session: Structural471,pub r#through_sequence: u64,pub r#updates: Vec<Structural472>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSessionProjection1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.session-projection@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSessionRenameInput1Type { pub r#session_id: String,pub r#title: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSessionRenameInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.session-rename-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSessionResumeInput1Type { pub r#after_sequence: Option<u64>,pub r#session_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSessionResumeInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.session-resume-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural531 { pub r#session_id: String,pub r#title: Option<String>,pub r#working_directory: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural536 { r#Error,r#Info,r#Warning, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural535 { pub r#code: String,pub r#message: String,pub r#resource: Option<String>,pub r#severity: Structural536, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural534 { pub r#diagnostic: Structural535, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural539 { pub r#fraction: Option<f64>,pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural544 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural545 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural546 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural547 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural548 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural549 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural550 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural551 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural552 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural553 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural554 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural555 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural556 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural557 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural543 { r#Cancelled,r#Closed,r#Conflict(Structural544),r#Disconnected,r#Failed(Structural545),r#InvalidInput(Structural546),r#InvalidPath(Structural547),r#InvalidResponse(Structural548),r#NotFound(Structural549),r#PermissionDenied(Structural550),r#SchemaMismatch(Structural551),r#StaleReference(Structural552),r#SubscriptionCapacity,r#TransactionConflict(Structural553),r#Unauthenticated(Structural554),r#UnknownValue(Structural555),r#UnsupportedCapability(Structural556),r#UnsupportedSnapshotPolicy(Structural557), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural542 { pub r#error: Structural543, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural541 { r#Cancelled,r#Completed,r#Failed(Structural542),r#Pending,r#Running, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural540 { pub r#state: Structural541, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural558 { pub r#call_id: String,pub r#callable_id: String,pub r#input: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural561 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural562 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural563 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural564 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural565 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural566 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural567 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural568 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural569 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural570 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural571 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural572 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural573 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural574 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural560 { r#Cancelled,r#Closed,r#Conflict(Structural561),r#Disconnected,r#Failed(Structural562),r#InvalidInput(Structural563),r#InvalidPath(Structural564),r#InvalidResponse(Structural565),r#NotFound(Structural566),r#PermissionDenied(Structural567),r#SchemaMismatch(Structural568),r#StaleReference(Structural569),r#SubscriptionCapacity,r#TransactionConflict(Structural570),r#Unauthenticated(Structural571),r#UnknownValue(Structural572),r#UnsupportedCapability(Structural573),r#UnsupportedSnapshotPolicy(Structural574), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural559 { pub r#call_id: String,pub r#error: Structural560, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural575 { pub r#call_id: String,pub r#output: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural538 { r#Progress(Structural539),r#State(Structural540),r#ToolCall(Structural558),r#ToolFailed(Structural559),r#ToolResult(Structural575), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural537 { pub r#execution_id: String,pub r#update: Structural538, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural579 { pub r#data: phenix_core::Bytes,pub r#mime_type: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural580 { pub r#mime_type: Option<String>,pub r#text: Option<String>,pub r#uri: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural581 { pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural578 { r#Image(Structural579),r#Resource(Structural580),r#Text(Structural581), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural582 { r#Assistant,r#User, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural577 { pub r#content: Vec<Structural578>,pub r#role: Structural582, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural576 { pub r#message: Structural577, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural583 { pub r#title: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural587 { pub r#id: String,pub r#new_count: u64,pub r#new_start: u64,pub r#old_count: u64,pub r#old_start: u64,pub r#unified_diff: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural586 { pub r#conflict: Option<String>,pub r#expected_version: String,pub r#hunks: Vec<Structural587>,pub r#uri: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural589 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural588 { r#Accepted,r#Conflicted(Structural589),r#Pending,r#Rejected, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural585 { pub r#execution_id: String,pub r#files: Vec<Structural586>,pub r#id: String,pub r#revision: u64,pub r#session_id: String,pub r#state: Structural588, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural584 { pub r#review: Structural585, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural590 { pub r#execution_id: String,pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural533 { r#Closed,r#Diagnostic(Structural534),r#Execution(Structural537),r#Message(Structural576),r#Renamed(Structural583),r#Review(Structural584),r#TextDelta(Structural590), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural532 { pub r#sequence: u64,pub r#session_id: String,pub r#update: Structural533, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSessionSnapshot1Type { pub r#session: Structural531,pub r#through_sequence: u64,pub r#updates: Vec<Structural532>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSessionSnapshot1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.session-snapshot@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural594 { r#Error,r#Info,r#Warning, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural593 { pub r#code: String,pub r#message: String,pub r#resource: Option<String>,pub r#severity: Structural594, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural592 { pub r#diagnostic: Structural593, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural597 { pub r#fraction: Option<f64>,pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural602 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural603 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural604 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural605 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural606 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural607 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural608 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural609 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural610 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural611 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural612 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural613 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural614 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural615 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural601 { r#Cancelled,r#Closed,r#Conflict(Structural602),r#Disconnected,r#Failed(Structural603),r#InvalidInput(Structural604),r#InvalidPath(Structural605),r#InvalidResponse(Structural606),r#NotFound(Structural607),r#PermissionDenied(Structural608),r#SchemaMismatch(Structural609),r#StaleReference(Structural610),r#SubscriptionCapacity,r#TransactionConflict(Structural611),r#Unauthenticated(Structural612),r#UnknownValue(Structural613),r#UnsupportedCapability(Structural614),r#UnsupportedSnapshotPolicy(Structural615), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural600 { pub r#error: Structural601, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural599 { r#Cancelled,r#Completed,r#Failed(Structural600),r#Pending,r#Running, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural598 { pub r#state: Structural599, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural616 { pub r#call_id: String,pub r#callable_id: String,pub r#input: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural619 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural620 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural621 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural622 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural623 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural624 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural625 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural626 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural627 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural628 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural629 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural630 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural631 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural632 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural618 { r#Cancelled,r#Closed,r#Conflict(Structural619),r#Disconnected,r#Failed(Structural620),r#InvalidInput(Structural621),r#InvalidPath(Structural622),r#InvalidResponse(Structural623),r#NotFound(Structural624),r#PermissionDenied(Structural625),r#SchemaMismatch(Structural626),r#StaleReference(Structural627),r#SubscriptionCapacity,r#TransactionConflict(Structural628),r#Unauthenticated(Structural629),r#UnknownValue(Structural630),r#UnsupportedCapability(Structural631),r#UnsupportedSnapshotPolicy(Structural632), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural617 { pub r#call_id: String,pub r#error: Structural618, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural633 { pub r#call_id: String,pub r#output: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural596 { r#Progress(Structural597),r#State(Structural598),r#ToolCall(Structural616),r#ToolFailed(Structural617),r#ToolResult(Structural633), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural595 { pub r#execution_id: String,pub r#update: Structural596, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural637 { pub r#data: phenix_core::Bytes,pub r#mime_type: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural638 { pub r#mime_type: Option<String>,pub r#text: Option<String>,pub r#uri: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural639 { pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural636 { r#Image(Structural637),r#Resource(Structural638),r#Text(Structural639), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural640 { r#Assistant,r#User, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural635 { pub r#content: Vec<Structural636>,pub r#role: Structural640, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural634 { pub r#message: Structural635, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural641 { pub r#title: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural645 { pub r#id: String,pub r#new_count: u64,pub r#new_start: u64,pub r#old_count: u64,pub r#old_start: u64,pub r#unified_diff: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural644 { pub r#conflict: Option<String>,pub r#expected_version: String,pub r#hunks: Vec<Structural645>,pub r#uri: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural647 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural646 { r#Accepted,r#Conflicted(Structural647),r#Pending,r#Rejected, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural643 { pub r#execution_id: String,pub r#files: Vec<Structural644>,pub r#id: String,pub r#revision: u64,pub r#session_id: String,pub r#state: Structural646, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural642 { pub r#review: Structural643, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural648 { pub r#execution_id: String,pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural591 { r#Closed,r#Diagnostic(Structural592),r#Execution(Structural595),r#Message(Structural634),r#Renamed(Structural641),r#Review(Structural642),r#TextDelta(Structural648), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSessionUpdate1Type { pub r#sequence: u64,pub r#session_id: String,pub r#update: Structural591, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSessionUpdate1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.session-update@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural650 { pub r#message: String,pub r#schema: phenix_core::PhenixValue,pub r#session_id: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural652 { pub r#value: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural651 { r#Accepted(Structural652),r#Cancelled,r#Declined, }
#[derive(Clone, Debug, PartialEq)]
pub struct Callable653(pub phenix_core::CallableRef);
impl phenix_core::ValueCodec for Callable653 { fn phenix_type() -> phenix_core::PhenixSchema { phenix_core::PhenixSchema::Callable { contract: phenix_core::ContractId::parse("phenix.application.elicitation@1").expect("generated callable contract is valid"), input: Box::new(<Structural650 as phenix_core::HasPhenixSchema>::phenix_schema()), output: Box::new(<Structural651 as phenix_core::HasPhenixSchema>::phenix_schema()) } } fn to_value(&self) -> phenix_core::PhenixValue { phenix_core::PhenixValue::Callable(self.0.clone()) } fn from_value(value: &phenix_core::PhenixValue) -> Result<Self, phenix_core::ValueError> { <Self as phenix_core::ValueCodec>::phenix_type().parse(value)?; match value { phenix_core::PhenixValue::Callable(reference) => Ok(Self(reference.clone())), _ => unreachable!("validated callable value"), } } }
impl From<&Callable653> for phenix_core::PhenixValue { fn from(value: &Callable653) -> Self { <Callable653 as phenix_core::ValueCodec>::to_value(value) } }
impl TryFrom<phenix_core::Exact<&phenix_core::PhenixValue>> for Callable653 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Exact<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::from_value(value.0) } }
impl TryFrom<phenix_core::Project<&phenix_core::PhenixValue>> for Callable653 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Project<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::project_from_value(value.0) } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural654 { pub r#call_id: String,pub r#description: String,pub r#execution_id: String,pub r#session_id: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural655 { r#AllowOnce,r#Cancelled,r#Deny, }
#[derive(Clone, Debug, PartialEq)]
pub struct Callable656(pub phenix_core::CallableRef);
impl phenix_core::ValueCodec for Callable656 { fn phenix_type() -> phenix_core::PhenixSchema { phenix_core::PhenixSchema::Callable { contract: phenix_core::ContractId::parse("phenix.application.permission@1").expect("generated callable contract is valid"), input: Box::new(<Structural654 as phenix_core::HasPhenixSchema>::phenix_schema()), output: Box::new(<Structural655 as phenix_core::HasPhenixSchema>::phenix_schema()) } } fn to_value(&self) -> phenix_core::PhenixValue { phenix_core::PhenixValue::Callable(self.0.clone()) } fn from_value(value: &phenix_core::PhenixValue) -> Result<Self, phenix_core::ValueError> { <Self as phenix_core::ValueCodec>::phenix_type().parse(value)?; match value { phenix_core::PhenixValue::Callable(reference) => Ok(Self(reference.clone())), _ => unreachable!("validated callable value"), } } }
impl From<&Callable656> for phenix_core::PhenixValue { fn from(value: &Callable656) -> Self { <Callable656 as phenix_core::ValueCodec>::to_value(value) } }
impl TryFrom<phenix_core::Exact<&phenix_core::PhenixValue>> for Callable656 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Exact<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::from_value(value.0) } }
impl TryFrom<phenix_core::Project<&phenix_core::PhenixValue>> for Callable656 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Project<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::project_from_value(value.0) } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural649 { pub r#elicitation: Option<Callable653>,pub r#permission: Option<Callable656>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSetInteractionHandlersInput1Type { pub r#handlers: Structural649, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSetInteractionHandlersInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.set-interaction-handlers-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeSeverity1Type { r#Error,r#Info,r#Warning, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSeverity1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.severity@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSkillActivateInput1Type { pub r#session_id: String,pub r#skill_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSkillActivateInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.skill-activate-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSkillInfo1Type { pub r#active: bool,pub r#description: String,pub r#id: String,pub r#name: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSkillInfo1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.skill-info@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural657 { pub r#active: bool,pub r#description: String,pub r#id: String,pub r#name: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSkills1Type { pub r#skills: Vec<Structural657>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSkills1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.skills@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeStopReason1Type { r#Cancelled,r#EndTurn,r#MaxTokens,r#Refused, }
impl phenix_core::PhenixContract for PhenixApplicationTypeStopReason1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.stop-reason@1").expect("generated contract id is valid") } }
pub fn type_schemas() -> std::collections::BTreeMap<phenix_core::ContractId, phenix_core::PhenixSchema> { std::collections::BTreeMap::from([
(<PhenixApplicationError1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationError1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeAcknowledged1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeAcknowledged1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeAuthenticateInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeAuthenticateInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeAuthenticationMethod1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeAuthenticationMethod1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeAuthenticationMethods1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeAuthenticationMethods1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeAuthenticationResult1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeAuthenticationResult1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeCallableInfo1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeCallableInfo1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeCallableInvocationResult1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeCallableInvocationResult1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeCallableInvocation1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeCallableInvocation1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeCallableInvokeInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeCallableInvokeInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeCallableResult1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeCallableResult1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeCallables1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeCallables1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeCapabilityList1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeCapabilityList1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeClientToolAddInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeClientToolAddInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeClientToolAdmission1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeClientToolAdmission1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeClientToolDefinition1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeClientToolDefinition1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeClientToolRemoveInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeClientToolRemoveInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeContent1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeContent1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeDiagnostic1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeDiagnostic1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeDiagnostics1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeDiagnostics1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeElicitationRequest1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeElicitationRequest1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeElicitationResponse1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeElicitationResponse1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeEmpty1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeEmpty1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeExecutionChange1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeExecutionChange1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeExecutionInfo1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeExecutionInfo1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeExecutionInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeExecutionInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeExecutionState1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeExecutionState1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeExecutionTree1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeExecutionTree1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeExecutionUpdate1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeExecutionUpdate1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeInteractionHandlers1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeInteractionHandlers1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeLogPage1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeLogPage1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeLogQueryInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeLogQueryInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeLogRecord1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeLogRecord1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeLogReferenceContent1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeLogReferenceContent1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeLogReferenceInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeLogReferenceInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeMessageRole1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeMessageRole1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeMessage1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeMessage1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeObservableAddress1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeObservableAddress1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeObservableChange1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeObservableChange1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeObservableDelivery1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeObservableDelivery1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeObservableGetInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeObservableGetInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeObservableInitial1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeObservableInitial1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeObservableList1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeObservableList1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeObservableMode1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeObservableMode1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeObservablePathSegment1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeObservablePathSegment1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeObservablePath1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeObservablePath1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeObservablePayload1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeObservablePayload1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeObservableRemove1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeObservableRemove1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeObservableReplace1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeObservableReplace1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeObservableResource1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeObservableResource1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeObservableScope1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeObservableScope1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeObservableSnapshotPolicy1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeObservableSnapshotPolicy1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeObservableSplice1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeObservableSplice1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeObservableSubscribeInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeObservableSubscribeInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeObservableSubscriptionResult1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeObservableSubscriptionResult1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeObservableUnsubscribeInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeObservableUnsubscribeInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeObservableValue1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeObservableValue1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypePageInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypePageInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypePermissionRequest1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypePermissionRequest1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypePermissionResponse1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypePermissionResponse1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypePromptInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypePromptInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypePromptResult1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypePromptResult1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeProvenance1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeProvenance1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeReviewDecisionInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeReviewDecisionInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeReviewDecision1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeReviewDecision1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeReviewFile1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeReviewFile1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeReviewHunk1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeReviewHunk1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeReviewRecord1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeReviewRecord1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeReviewState1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeReviewState1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSdkValue1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSdkValue1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSelectionDefaultSelectInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSelectionDefaultSelectInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSelectionInfo1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSelectionInfo1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSelectionPresentation1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSelectionPresentation1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSelectionSelectInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSelectionSelectInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSelections1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSelections1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionChange1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionChange1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionCreateInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionCreateInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionInfo1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionInfo1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionLineage1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionLineage1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionList1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionList1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionProjectionState1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionProjectionState1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionProjection1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionProjection1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionRenameInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionRenameInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionResumeInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionResumeInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionSnapshot1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionSnapshot1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionUpdate1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionUpdate1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSetInteractionHandlersInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSetInteractionHandlersInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSeverity1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSeverity1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSkillActivateInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSkillActivateInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSkillInfo1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSkillInfo1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSkills1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSkills1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeStopReason1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeStopReason1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
]) }
pub struct PhenixApplicationAuthenticate1Operation;
impl phenix_application_interface::Operation for PhenixApplicationAuthenticate1Operation { const ID: &'static str = "phenix.application.authenticate@1"; const CAPABILITY: &'static str = "phenix.application.capability.authentication@1"; type Input = PhenixApplicationTypeAuthenticateInput1Type; type Output = PhenixApplicationTypeAuthenticationResult1Type; }
impl PhenixApplicationAuthenticate1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeAuthenticateInput1Type) -> Result<PhenixApplicationTypeAuthenticationResult1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationAuthenticationList1Operation;
impl phenix_application_interface::Operation for PhenixApplicationAuthenticationList1Operation { const ID: &'static str = "phenix.application.authentication-list@1"; const CAPABILITY: &'static str = "phenix.application.capability.authentication@1"; type Input = PhenixApplicationTypeEmpty1Type; type Output = PhenixApplicationTypeAuthenticationMethods1Type; }
impl PhenixApplicationAuthenticationList1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeEmpty1Type) -> Result<PhenixApplicationTypeAuthenticationMethods1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationCallableInvoke1Operation;
impl phenix_application_interface::Operation for PhenixApplicationCallableInvoke1Operation { const ID: &'static str = "phenix.application.callable-invoke@1"; const CAPABILITY: &'static str = "phenix.application.capability.callables@1"; type Input = PhenixApplicationTypeCallableInvokeInput1Type; type Output = PhenixApplicationTypeCallableResult1Type; }
impl PhenixApplicationCallableInvoke1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeCallableInvokeInput1Type) -> Result<PhenixApplicationTypeCallableResult1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationCallableList1Operation;
impl phenix_application_interface::Operation for PhenixApplicationCallableList1Operation { const ID: &'static str = "phenix.application.callable-list@1"; const CAPABILITY: &'static str = "phenix.application.capability.callables@1"; type Input = PhenixApplicationTypeSessionInput1Type; type Output = PhenixApplicationTypeCallables1Type; }
impl PhenixApplicationCallableList1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeSessionInput1Type) -> Result<PhenixApplicationTypeCallables1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationCancel1Operation;
impl phenix_application_interface::Operation for PhenixApplicationCancel1Operation { const ID: &'static str = "phenix.application.cancel@1"; const CAPABILITY: &'static str = "phenix.application.capability.prompt@1"; type Input = PhenixApplicationTypeSessionInput1Type; type Output = PhenixApplicationTypeAcknowledged1Type; }
impl PhenixApplicationCancel1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeSessionInput1Type) -> Result<PhenixApplicationTypeAcknowledged1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationCapabilities1Operation;
impl phenix_application_interface::Operation for PhenixApplicationCapabilities1Operation { const ID: &'static str = "phenix.application.capabilities@1"; const CAPABILITY: &'static str = "phenix.application.capability.discovery@1"; type Input = PhenixApplicationTypeEmpty1Type; type Output = PhenixApplicationTypeCapabilityList1Type; }
impl PhenixApplicationCapabilities1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeEmpty1Type) -> Result<PhenixApplicationTypeCapabilityList1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationCapabilityInvoke1Operation;
impl phenix_application_interface::Operation for PhenixApplicationCapabilityInvoke1Operation { const ID: &'static str = "phenix.application.capability-invoke@1"; const CAPABILITY: &'static str = "phenix.application.capability.capabilities@1"; type Input = PhenixApplicationTypeCallableInvocation1Type; type Output = PhenixApplicationTypeCallableInvocationResult1Type; }
impl PhenixApplicationCapabilityInvoke1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeCallableInvocation1Type) -> Result<PhenixApplicationTypeCallableInvocationResult1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationClientToolAdd1Operation;
impl phenix_application_interface::Operation for PhenixApplicationClientToolAdd1Operation { const ID: &'static str = "phenix.application.client-tool-add@1"; const CAPABILITY: &'static str = "phenix.application.capability.client-tools@1"; type Input = PhenixApplicationTypeClientToolAddInput1Type; type Output = PhenixApplicationTypeClientToolAdmission1Type; }
impl PhenixApplicationClientToolAdd1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeClientToolAddInput1Type) -> Result<PhenixApplicationTypeClientToolAdmission1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationClientToolRemove1Operation;
impl phenix_application_interface::Operation for PhenixApplicationClientToolRemove1Operation { const ID: &'static str = "phenix.application.client-tool-remove@1"; const CAPABILITY: &'static str = "phenix.application.capability.client-tools@1"; type Input = PhenixApplicationTypeClientToolRemoveInput1Type; type Output = PhenixApplicationTypeAcknowledged1Type; }
impl PhenixApplicationClientToolRemove1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeClientToolRemoveInput1Type) -> Result<PhenixApplicationTypeAcknowledged1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationDiagnostics1Operation;
impl phenix_application_interface::Operation for PhenixApplicationDiagnostics1Operation { const ID: &'static str = "phenix.application.diagnostics@1"; const CAPABILITY: &'static str = "phenix.application.capability.diagnostics@1"; type Input = PhenixApplicationTypeEmpty1Type; type Output = PhenixApplicationTypeDiagnostics1Type; }
impl PhenixApplicationDiagnostics1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeEmpty1Type) -> Result<PhenixApplicationTypeDiagnostics1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationExecutionProvenance1Operation;
impl phenix_application_interface::Operation for PhenixApplicationExecutionProvenance1Operation { const ID: &'static str = "phenix.application.execution-provenance@1"; const CAPABILITY: &'static str = "phenix.application.capability.inspection@1"; type Input = PhenixApplicationTypeExecutionInput1Type; type Output = PhenixApplicationTypeProvenance1Type; }
impl PhenixApplicationExecutionProvenance1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeExecutionInput1Type) -> Result<PhenixApplicationTypeProvenance1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationExecutionTree1Operation;
impl phenix_application_interface::Operation for PhenixApplicationExecutionTree1Operation { const ID: &'static str = "phenix.application.execution-tree@1"; const CAPABILITY: &'static str = "phenix.application.capability.inspection@1"; type Input = PhenixApplicationTypeSessionInput1Type; type Output = PhenixApplicationTypeExecutionTree1Type; }
impl PhenixApplicationExecutionTree1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeSessionInput1Type) -> Result<PhenixApplicationTypeExecutionTree1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationInteractionHandlersSet1Operation;
impl phenix_application_interface::Operation for PhenixApplicationInteractionHandlersSet1Operation { const ID: &'static str = "phenix.application.interaction-handlers-set@1"; const CAPABILITY: &'static str = "phenix.application.capability.interaction@1"; type Input = PhenixApplicationTypeSetInteractionHandlersInput1Type; type Output = PhenixApplicationTypeAcknowledged1Type; }
impl PhenixApplicationInteractionHandlersSet1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeSetInteractionHandlersInput1Type) -> Result<PhenixApplicationTypeAcknowledged1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationLogQuery1Operation;
impl phenix_application_interface::Operation for PhenixApplicationLogQuery1Operation { const ID: &'static str = "phenix.application.log-query@1"; const CAPABILITY: &'static str = "phenix.application.capability.logs@1"; type Input = PhenixApplicationTypeLogQueryInput1Type; type Output = PhenixApplicationTypeLogPage1Type; }
impl PhenixApplicationLogQuery1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeLogQueryInput1Type) -> Result<PhenixApplicationTypeLogPage1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationLogReferenceRead1Operation;
impl phenix_application_interface::Operation for PhenixApplicationLogReferenceRead1Operation { const ID: &'static str = "phenix.application.log-reference-read@1"; const CAPABILITY: &'static str = "phenix.application.capability.logs@1"; type Input = PhenixApplicationTypeLogReferenceInput1Type; type Output = PhenixApplicationTypeLogReferenceContent1Type; }
impl PhenixApplicationLogReferenceRead1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeLogReferenceInput1Type) -> Result<PhenixApplicationTypeLogReferenceContent1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationObservableGet1Operation;
impl phenix_application_interface::Operation for PhenixApplicationObservableGet1Operation { const ID: &'static str = "phenix.application.observable-get@1"; const CAPABILITY: &'static str = "phenix.application.capability.observables@1"; type Input = PhenixApplicationTypeObservableGetInput1Type; type Output = PhenixApplicationTypeObservableValue1Type; }
impl PhenixApplicationObservableGet1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeObservableGetInput1Type) -> Result<PhenixApplicationTypeObservableValue1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationObservableList1Operation;
impl phenix_application_interface::Operation for PhenixApplicationObservableList1Operation { const ID: &'static str = "phenix.application.observable-list@1"; const CAPABILITY: &'static str = "phenix.application.capability.observables@1"; type Input = PhenixApplicationTypeEmpty1Type; type Output = PhenixApplicationTypeObservableList1Type; }
impl PhenixApplicationObservableList1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeEmpty1Type) -> Result<PhenixApplicationTypeObservableList1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationObservableSubscribe1Operation;
impl phenix_application_interface::Operation for PhenixApplicationObservableSubscribe1Operation { const ID: &'static str = "phenix.application.observable-subscribe@1"; const CAPABILITY: &'static str = "phenix.application.capability.observables@1"; type Input = PhenixApplicationTypeObservableSubscribeInput1Type; type Output = PhenixApplicationTypeObservableSubscriptionResult1Type; }
impl PhenixApplicationObservableSubscribe1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeObservableSubscribeInput1Type) -> Result<PhenixApplicationTypeObservableSubscriptionResult1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationObservableUnsubscribe1Operation;
impl phenix_application_interface::Operation for PhenixApplicationObservableUnsubscribe1Operation { const ID: &'static str = "phenix.application.observable-unsubscribe@1"; const CAPABILITY: &'static str = "phenix.application.capability.observables@1"; type Input = PhenixApplicationTypeObservableUnsubscribeInput1Type; type Output = PhenixApplicationTypeAcknowledged1Type; }
impl PhenixApplicationObservableUnsubscribe1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeObservableUnsubscribeInput1Type) -> Result<PhenixApplicationTypeAcknowledged1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationPrompt1Operation;
impl phenix_application_interface::Operation for PhenixApplicationPrompt1Operation { const ID: &'static str = "phenix.application.prompt@1"; const CAPABILITY: &'static str = "phenix.application.capability.prompt@1"; type Input = PhenixApplicationTypePromptInput1Type; type Output = PhenixApplicationTypePromptResult1Type; }
impl PhenixApplicationPrompt1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypePromptInput1Type) -> Result<PhenixApplicationTypePromptResult1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationReviewDecide1Operation;
impl phenix_application_interface::Operation for PhenixApplicationReviewDecide1Operation { const ID: &'static str = "phenix.application.review-decide@1"; const CAPABILITY: &'static str = "phenix.application.capability.review@1"; type Input = PhenixApplicationTypeReviewDecisionInput1Type; type Output = PhenixApplicationTypeReviewRecord1Type; }
impl PhenixApplicationReviewDecide1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeReviewDecisionInput1Type) -> Result<PhenixApplicationTypeReviewRecord1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationSdkGet1Operation;
impl phenix_application_interface::Operation for PhenixApplicationSdkGet1Operation { const ID: &'static str = "phenix.application.sdk-get@1"; const CAPABILITY: &'static str = "phenix.application.capability.sdk@1"; type Input = PhenixApplicationTypeEmpty1Type; type Output = PhenixApplicationTypeSdkValue1Type; }
impl PhenixApplicationSdkGet1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeEmpty1Type) -> Result<PhenixApplicationTypeSdkValue1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationSelectionDefaultList1Operation;
impl phenix_application_interface::Operation for PhenixApplicationSelectionDefaultList1Operation { const ID: &'static str = "phenix.application.selection-default-list@1"; const CAPABILITY: &'static str = "phenix.application.capability.routing@1"; type Input = PhenixApplicationTypeEmpty1Type; type Output = PhenixApplicationTypeSelections1Type; }
impl PhenixApplicationSelectionDefaultList1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeEmpty1Type) -> Result<PhenixApplicationTypeSelections1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationSelectionDefaultSelect1Operation;
impl phenix_application_interface::Operation for PhenixApplicationSelectionDefaultSelect1Operation { const ID: &'static str = "phenix.application.selection-default-select@1"; const CAPABILITY: &'static str = "phenix.application.capability.routing@1"; type Input = PhenixApplicationTypeSelectionDefaultSelectInput1Type; type Output = PhenixApplicationTypeSelections1Type; }
impl PhenixApplicationSelectionDefaultSelect1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeSelectionDefaultSelectInput1Type) -> Result<PhenixApplicationTypeSelections1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationSelectionList1Operation;
impl phenix_application_interface::Operation for PhenixApplicationSelectionList1Operation { const ID: &'static str = "phenix.application.selection-list@1"; const CAPABILITY: &'static str = "phenix.application.capability.routing@1"; type Input = PhenixApplicationTypeSessionInput1Type; type Output = PhenixApplicationTypeSelections1Type; }
impl PhenixApplicationSelectionList1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeSessionInput1Type) -> Result<PhenixApplicationTypeSelections1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationSelectionSelect1Operation;
impl phenix_application_interface::Operation for PhenixApplicationSelectionSelect1Operation { const ID: &'static str = "phenix.application.selection-select@1"; const CAPABILITY: &'static str = "phenix.application.capability.routing@1"; type Input = PhenixApplicationTypeSelectionSelectInput1Type; type Output = PhenixApplicationTypeSelections1Type; }
impl PhenixApplicationSelectionSelect1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeSelectionSelectInput1Type) -> Result<PhenixApplicationTypeSelections1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationSessionClose1Operation;
impl phenix_application_interface::Operation for PhenixApplicationSessionClose1Operation { const ID: &'static str = "phenix.application.session-close@1"; const CAPABILITY: &'static str = "phenix.application.capability.sessions@1"; type Input = PhenixApplicationTypeSessionInput1Type; type Output = PhenixApplicationTypeAcknowledged1Type; }
impl PhenixApplicationSessionClose1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeSessionInput1Type) -> Result<PhenixApplicationTypeAcknowledged1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationSessionCreate1Operation;
impl phenix_application_interface::Operation for PhenixApplicationSessionCreate1Operation { const ID: &'static str = "phenix.application.session-create@1"; const CAPABILITY: &'static str = "phenix.application.capability.sessions@1"; type Input = PhenixApplicationTypeSessionCreateInput1Type; type Output = PhenixApplicationTypeSessionInfo1Type; }
impl PhenixApplicationSessionCreate1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeSessionCreateInput1Type) -> Result<PhenixApplicationTypeSessionInfo1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationSessionLineage1Operation;
impl phenix_application_interface::Operation for PhenixApplicationSessionLineage1Operation { const ID: &'static str = "phenix.application.session-lineage@1"; const CAPABILITY: &'static str = "phenix.application.capability.lineage@1"; type Input = PhenixApplicationTypeSessionInput1Type; type Output = PhenixApplicationTypeSessionLineage1Type; }
impl PhenixApplicationSessionLineage1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeSessionInput1Type) -> Result<PhenixApplicationTypeSessionLineage1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationSessionList1Operation;
impl phenix_application_interface::Operation for PhenixApplicationSessionList1Operation { const ID: &'static str = "phenix.application.session-list@1"; const CAPABILITY: &'static str = "phenix.application.capability.session-list@1"; type Input = PhenixApplicationTypePageInput1Type; type Output = PhenixApplicationTypeSessionList1Type; }
impl PhenixApplicationSessionList1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypePageInput1Type) -> Result<PhenixApplicationTypeSessionList1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationSessionRename1Operation;
impl phenix_application_interface::Operation for PhenixApplicationSessionRename1Operation { const ID: &'static str = "phenix.application.session-rename@1"; const CAPABILITY: &'static str = "phenix.application.capability.session-rename@1"; type Input = PhenixApplicationTypeSessionRenameInput1Type; type Output = PhenixApplicationTypeSessionInfo1Type; }
impl PhenixApplicationSessionRename1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeSessionRenameInput1Type) -> Result<PhenixApplicationTypeSessionInfo1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationSessionResume1Operation;
impl phenix_application_interface::Operation for PhenixApplicationSessionResume1Operation { const ID: &'static str = "phenix.application.session-resume@1"; const CAPABILITY: &'static str = "phenix.application.capability.session-resume@1"; type Input = PhenixApplicationTypeSessionResumeInput1Type; type Output = PhenixApplicationTypeSessionSnapshot1Type; }
impl PhenixApplicationSessionResume1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeSessionResumeInput1Type) -> Result<PhenixApplicationTypeSessionSnapshot1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationSkillActivate1Operation;
impl phenix_application_interface::Operation for PhenixApplicationSkillActivate1Operation { const ID: &'static str = "phenix.application.skill-activate@1"; const CAPABILITY: &'static str = "phenix.application.capability.skills@1"; type Input = PhenixApplicationTypeSkillActivateInput1Type; type Output = PhenixApplicationTypeSkills1Type; }
impl PhenixApplicationSkillActivate1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeSkillActivateInput1Type) -> Result<PhenixApplicationTypeSkills1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationSkillList1Operation;
impl phenix_application_interface::Operation for PhenixApplicationSkillList1Operation { const ID: &'static str = "phenix.application.skill-list@1"; const CAPABILITY: &'static str = "phenix.application.capability.skills@1"; type Input = PhenixApplicationTypeSessionInput1Type; type Output = PhenixApplicationTypeSkills1Type; }
impl PhenixApplicationSkillList1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeSessionInput1Type) -> Result<PhenixApplicationTypeSkills1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub type PhenixApplicationExecutionUpdate1Event = PhenixApplicationTypeExecutionUpdate1Type;
pub const PHENIXAPPLICATIONEXECUTIONUPDATE1EVENT: &str = "phenix.application.execution-update@1";
pub type PhenixApplicationObservableUpdate1Event = PhenixApplicationTypeObservableDelivery1Type;
pub const PHENIXAPPLICATIONOBSERVABLEUPDATE1EVENT: &str = "phenix.application.observable-update@1";
pub type PhenixApplicationSessionUpdate1Event = PhenixApplicationTypeSessionUpdate1Type;
pub const PHENIXAPPLICATIONSESSIONUPDATE1EVENT: &str = "phenix.application.session-update@1";
pub type PhenixApplicationCapabilityCall1CallbackRequest = PhenixApplicationTypeCallableInvocation1Type;
pub const PHENIXAPPLICATIONCAPABILITYCALL1CALLBACKREQUEST: &str = "phenix.application.capability-call@1";
pub type PhenixApplicationCapabilityCall1CallbackResponse = PhenixApplicationTypeCallableInvocationResult1Type;
pub const PHENIXAPPLICATIONCAPABILITYCALL1CALLBACKRESPONSE: &str = "phenix.application.capability-call@1";
pub type PhenixApplicationElicitation1CallbackRequest = PhenixApplicationTypeElicitationRequest1Type;
pub const PHENIXAPPLICATIONELICITATION1CALLBACKREQUEST: &str = "phenix.application.elicitation@1";
pub type PhenixApplicationElicitation1CallbackResponse = PhenixApplicationTypeElicitationResponse1Type;
pub const PHENIXAPPLICATIONELICITATION1CALLBACKRESPONSE: &str = "phenix.application.elicitation@1";
pub type PhenixApplicationPermission1CallbackRequest = PhenixApplicationTypePermissionRequest1Type;
pub const PHENIXAPPLICATIONPERMISSION1CALLBACKREQUEST: &str = "phenix.application.permission@1";
pub type PhenixApplicationPermission1CallbackResponse = PhenixApplicationTypePermissionResponse1Type;
pub const PHENIXAPPLICATIONPERMISSION1CALLBACKRESPONSE: &str = "phenix.application.permission@1";
pub struct PhenixApplicationCapabilityAuthentication1Capability; impl PhenixApplicationCapabilityAuthentication1Capability { pub const ID: &'static str = "phenix.application.capability.authentication@1"; }
pub struct PhenixApplicationCapabilityCallables1Capability; impl PhenixApplicationCapabilityCallables1Capability { pub const ID: &'static str = "phenix.application.capability.callables@1"; }
pub struct PhenixApplicationCapabilityCapabilities1Capability; impl PhenixApplicationCapabilityCapabilities1Capability { pub const ID: &'static str = "phenix.application.capability.capabilities@1"; }
pub struct PhenixApplicationCapabilityClientTools1Capability; impl PhenixApplicationCapabilityClientTools1Capability { pub const ID: &'static str = "phenix.application.capability.client-tools@1"; }
pub struct PhenixApplicationCapabilityDiagnostics1Capability; impl PhenixApplicationCapabilityDiagnostics1Capability { pub const ID: &'static str = "phenix.application.capability.diagnostics@1"; }
pub struct PhenixApplicationCapabilityDiscovery1Capability; impl PhenixApplicationCapabilityDiscovery1Capability { pub const ID: &'static str = "phenix.application.capability.discovery@1"; }
pub struct PhenixApplicationCapabilityElicitation1Capability; impl PhenixApplicationCapabilityElicitation1Capability { pub const ID: &'static str = "phenix.application.capability.elicitation@1"; }
pub struct PhenixApplicationCapabilityInspection1Capability; impl PhenixApplicationCapabilityInspection1Capability { pub const ID: &'static str = "phenix.application.capability.inspection@1"; }
pub struct PhenixApplicationCapabilityInteraction1Capability; impl PhenixApplicationCapabilityInteraction1Capability { pub const ID: &'static str = "phenix.application.capability.interaction@1"; }
pub struct PhenixApplicationCapabilityLineage1Capability; impl PhenixApplicationCapabilityLineage1Capability { pub const ID: &'static str = "phenix.application.capability.lineage@1"; }
pub struct PhenixApplicationCapabilityLogs1Capability; impl PhenixApplicationCapabilityLogs1Capability { pub const ID: &'static str = "phenix.application.capability.logs@1"; }
pub struct PhenixApplicationCapabilityObservables1Capability; impl PhenixApplicationCapabilityObservables1Capability { pub const ID: &'static str = "phenix.application.capability.observables@1"; }
pub struct PhenixApplicationCapabilityPermission1Capability; impl PhenixApplicationCapabilityPermission1Capability { pub const ID: &'static str = "phenix.application.capability.permission@1"; }
pub struct PhenixApplicationCapabilityPrompt1Capability; impl PhenixApplicationCapabilityPrompt1Capability { pub const ID: &'static str = "phenix.application.capability.prompt@1"; }
pub struct PhenixApplicationCapabilityReview1Capability; impl PhenixApplicationCapabilityReview1Capability { pub const ID: &'static str = "phenix.application.capability.review@1"; }
pub struct PhenixApplicationCapabilityRouting1Capability; impl PhenixApplicationCapabilityRouting1Capability { pub const ID: &'static str = "phenix.application.capability.routing@1"; }
pub struct PhenixApplicationCapabilitySdk1Capability; impl PhenixApplicationCapabilitySdk1Capability { pub const ID: &'static str = "phenix.application.capability.sdk@1"; }
pub struct PhenixApplicationCapabilitySessionList1Capability; impl PhenixApplicationCapabilitySessionList1Capability { pub const ID: &'static str = "phenix.application.capability.session-list@1"; }
pub struct PhenixApplicationCapabilitySessionRename1Capability; impl PhenixApplicationCapabilitySessionRename1Capability { pub const ID: &'static str = "phenix.application.capability.session-rename@1"; }
pub struct PhenixApplicationCapabilitySessionResume1Capability; impl PhenixApplicationCapabilitySessionResume1Capability { pub const ID: &'static str = "phenix.application.capability.session-resume@1"; }
pub struct PhenixApplicationCapabilitySessions1Capability; impl PhenixApplicationCapabilitySessions1Capability { pub const ID: &'static str = "phenix.application.capability.sessions@1"; }
pub struct PhenixApplicationCapabilitySkills1Capability; impl PhenixApplicationCapabilitySkills1Capability { pub const ID: &'static str = "phenix.application.capability.skills@1"; }

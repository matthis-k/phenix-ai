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
pub struct PhenixApplicationTypeAuthenticateInput1Type { pub r#method_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeAuthenticateInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.authenticate-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeAuthenticationMethod1Type { pub r#description: Option<String>,pub r#id: String,pub r#name: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeAuthenticationMethod1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.authentication-method@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural14 { pub r#description: Option<String>,pub r#id: String,pub r#name: String, }
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
pub struct PhenixApplicationTypeCapabilityInvokeInput1Type { pub r#callable: phenix_core::PhenixValue,pub r#input: phenix_core::PhenixValue, }
impl phenix_core::PhenixContract for PhenixApplicationTypeCapabilityInvokeInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.capability-invoke-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeCapabilityInvokeResult1Type { pub r#output: phenix_core::PhenixValue, }
impl phenix_core::PhenixContract for PhenixApplicationTypeCapabilityInvokeResult1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.capability-invoke-result@1").expect("generated contract id is valid") } }
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
pub enum PhenixApplicationTypeMessageRole1Type { r#Assistant,r#User, }
impl phenix_core::PhenixContract for PhenixApplicationTypeMessageRole1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.message-role@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural152 { pub r#data: phenix_core::Bytes,pub r#mime_type: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural153 { pub r#mime_type: Option<String>,pub r#text: Option<String>,pub r#uri: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural154 { pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural151 { r#Image(Structural152),r#Resource(Structural153),r#Text(Structural154), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural155 { r#Assistant,r#User, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeMessage1Type { pub r#content: Vec<Structural151>,pub r#role: Structural155, }
impl phenix_core::PhenixContract for PhenixApplicationTypeMessage1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.message@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeModelInfo1Type { pub r#description: Option<String>,pub r#id: String,pub r#name: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeModelInfo1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.model-info@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeModelSelectInput1Type { pub r#model_id: String,pub r#session_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeModelSelectInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.model-select-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural156 { pub r#description: Option<String>,pub r#id: String,pub r#name: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeModels1Type { pub r#available: Vec<Structural156>,pub r#selected: Option<String>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeModels1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.models@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural159 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural160 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural161 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural158 { r#Field(Structural159),r#Index(Structural160),r#MapKey(Structural161),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural157 { pub r#segments: Vec<Structural158>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableAddress1Type { pub r#path: Structural157,pub r#value_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableAddress1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-address@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural166 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural167 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural168 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural165 { r#Field(Structural166),r#Index(Structural167),r#MapKey(Structural168),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural164 { pub r#segments: Vec<Structural165>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural163 { pub r#path: Structural164, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural162 { pub r#change: Structural163, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural173 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural174 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural175 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural172 { r#Field(Structural173),r#Index(Structural174),r#MapKey(Structural175),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural171 { pub r#segments: Vec<Structural172>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural170 { pub r#path: Structural171,pub r#value: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural169 { pub r#change: Structural170, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural180 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural181 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural182 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural179 { r#Field(Structural180),r#Index(Structural181),r#MapKey(Structural182),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural178 { pub r#segments: Vec<Structural179>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural177 { pub r#delete_count: u64,pub r#inserted: Vec<phenix_core::PhenixValue>,pub r#path: Structural178,pub r#start: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural176 { pub r#change: Structural177, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeObservableChange1Type { r#Remove(Structural162),r#Replace(Structural169),r#Splice(Structural176), }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableChange1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-change@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural186 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural187 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural188 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural185 { r#Field(Structural186),r#Index(Structural187),r#MapKey(Structural188),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural184 { pub r#segments: Vec<Structural185>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural183 { pub r#path: Structural184,pub r#value_id: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural196 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural197 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural198 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural195 { r#Field(Structural196),r#Index(Structural197),r#MapKey(Structural198),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural194 { pub r#segments: Vec<Structural195>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural193 { pub r#path: Structural194, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural192 { pub r#change: Structural193, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural203 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural204 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural205 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural202 { r#Field(Structural203),r#Index(Structural204),r#MapKey(Structural205),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural201 { pub r#segments: Vec<Structural202>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural200 { pub r#path: Structural201,pub r#value: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural199 { pub r#change: Structural200, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural210 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural211 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural212 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural209 { r#Field(Structural210),r#Index(Structural211),r#MapKey(Structural212),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural208 { pub r#segments: Vec<Structural209>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural207 { pub r#delete_count: u64,pub r#inserted: Vec<phenix_core::PhenixValue>,pub r#path: Structural208,pub r#start: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural206 { pub r#change: Structural207, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural191 { r#Remove(Structural192),r#Replace(Structural199),r#Splice(Structural206), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural190 { pub r#changes: Vec<Structural191>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural213 { pub r#value: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural189 { r#Diff(Structural190),r#Full(Structural213), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableDelivery1Type { pub r#address: Structural183,pub r#commit_id: Option<u64>,pub r#from_version: u64,pub r#generation: u64,pub r#payload: Structural189,pub r#subscription_id: u64,pub r#value_id: String,pub r#version: u64, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableDelivery1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-delivery@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural216 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural217 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural218 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural215 { r#Field(Structural216),r#Index(Structural217),r#MapKey(Structural218),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural214 { pub r#segments: Vec<Structural215>, }
#[derive(Clone, Debug, PartialEq)]
pub struct Object219(pub phenix_core::ObjectRef);
impl phenix_core::ValueCodec for Object219 { fn phenix_type() -> phenix_core::PhenixSchema { phenix_core::PhenixSchema::Object { contract: phenix_core::ContractId::parse("phenix.observable@1").expect("generated object contract is valid") } } fn to_value(&self) -> phenix_core::PhenixValue { phenix_core::PhenixValue::Object(self.0.clone()) } fn from_value(value: &phenix_core::PhenixValue) -> Result<Self, phenix_core::ValueError> { <Self as phenix_core::ValueCodec>::phenix_type().parse(value)?; match value { phenix_core::PhenixValue::Object(reference) => Ok(Self(reference.clone())), _ => unreachable!("validated object value"), } } }
impl From<&Object219> for phenix_core::PhenixValue { fn from(value: &Object219) -> Self { <Object219 as phenix_core::ValueCodec>::to_value(value) } }
impl TryFrom<phenix_core::Exact<&phenix_core::PhenixValue>> for Object219 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Exact<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::from_value(value.0) } }
impl TryFrom<phenix_core::Project<&phenix_core::PhenixValue>> for Object219 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Project<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::project_from_value(value.0) } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableGetInput1Type { pub r#path: Structural214,pub r#reference: Object219, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableGetInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-get-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeObservableInitial1Type { r#Full,r#None, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableInitial1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-initial@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural223 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural224 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural225 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural222 { r#Field(Structural223),r#Index(Structural224),r#MapKey(Structural225),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural221 { pub r#segments: Vec<Structural222>, }
#[derive(Clone, Debug, PartialEq)]
pub struct Object226(pub phenix_core::ObjectRef);
impl phenix_core::ValueCodec for Object226 { fn phenix_type() -> phenix_core::PhenixSchema { phenix_core::PhenixSchema::Object { contract: phenix_core::ContractId::parse("phenix.observable@1").expect("generated object contract is valid") } } fn to_value(&self) -> phenix_core::PhenixValue { phenix_core::PhenixValue::Object(self.0.clone()) } fn from_value(value: &phenix_core::PhenixValue) -> Result<Self, phenix_core::ValueError> { <Self as phenix_core::ValueCodec>::phenix_type().parse(value)?; match value { phenix_core::PhenixValue::Object(reference) => Ok(Self(reference.clone())), _ => unreachable!("validated object value"), } } }
impl From<&Object226> for phenix_core::PhenixValue { fn from(value: &Object226) -> Self { <Object226 as phenix_core::ValueCodec>::to_value(value) } }
impl TryFrom<phenix_core::Exact<&phenix_core::PhenixValue>> for Object226 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Exact<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::from_value(value.0) } }
impl TryFrom<phenix_core::Project<&phenix_core::PhenixValue>> for Object226 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Project<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::project_from_value(value.0) } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural227 { r#CopyOnChange,r#CurrentOnly, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural220 { pub r#binding_path: Vec<String>,pub r#namespace: String,pub r#path: Structural221,pub r#reference: Object226,pub r#resource: String,pub r#schema: phenix_core::PhenixValue,pub r#snapshot_policy: Structural227,pub r#value_id: String,pub r#version: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableList1Type { pub r#resources: Vec<Structural220>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableList1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-list@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeObservableMode1Type { r#Diff,r#Full, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableMode1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-mode@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural228 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural229 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural230 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeObservablePathSegment1Type { r#Field(Structural228),r#Index(Structural229),r#MapKey(Structural230),r#OptionPayload,r#VariantPayload, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservablePathSegment1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-path-segment@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural232 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural233 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural234 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural231 { r#Field(Structural232),r#Index(Structural233),r#MapKey(Structural234),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservablePath1Type { pub r#segments: Vec<Structural231>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservablePath1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-path@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural241 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural242 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural243 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural240 { r#Field(Structural241),r#Index(Structural242),r#MapKey(Structural243),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural239 { pub r#segments: Vec<Structural240>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural238 { pub r#path: Structural239, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural237 { pub r#change: Structural238, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural248 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural249 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural250 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural247 { r#Field(Structural248),r#Index(Structural249),r#MapKey(Structural250),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural246 { pub r#segments: Vec<Structural247>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural245 { pub r#path: Structural246,pub r#value: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural244 { pub r#change: Structural245, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural255 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural256 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural257 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural254 { r#Field(Structural255),r#Index(Structural256),r#MapKey(Structural257),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural253 { pub r#segments: Vec<Structural254>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural252 { pub r#delete_count: u64,pub r#inserted: Vec<phenix_core::PhenixValue>,pub r#path: Structural253,pub r#start: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural251 { pub r#change: Structural252, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural236 { r#Remove(Structural237),r#Replace(Structural244),r#Splice(Structural251), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural235 { pub r#changes: Vec<Structural236>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural258 { pub r#value: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeObservablePayload1Type { r#Diff(Structural235),r#Full(Structural258), }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservablePayload1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-payload@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural261 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural262 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural263 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural260 { r#Field(Structural261),r#Index(Structural262),r#MapKey(Structural263),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural259 { pub r#segments: Vec<Structural260>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableRemove1Type { pub r#path: Structural259, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableRemove1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-remove@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural266 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural267 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural268 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural265 { r#Field(Structural266),r#Index(Structural267),r#MapKey(Structural268),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural264 { pub r#segments: Vec<Structural265>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableReplace1Type { pub r#path: Structural264,pub r#value: phenix_core::PhenixValue, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableReplace1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-replace@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural271 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural272 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural273 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural270 { r#Field(Structural271),r#Index(Structural272),r#MapKey(Structural273),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural269 { pub r#segments: Vec<Structural270>, }
#[derive(Clone, Debug, PartialEq)]
pub struct Object274(pub phenix_core::ObjectRef);
impl phenix_core::ValueCodec for Object274 { fn phenix_type() -> phenix_core::PhenixSchema { phenix_core::PhenixSchema::Object { contract: phenix_core::ContractId::parse("phenix.observable@1").expect("generated object contract is valid") } } fn to_value(&self) -> phenix_core::PhenixValue { phenix_core::PhenixValue::Object(self.0.clone()) } fn from_value(value: &phenix_core::PhenixValue) -> Result<Self, phenix_core::ValueError> { <Self as phenix_core::ValueCodec>::phenix_type().parse(value)?; match value { phenix_core::PhenixValue::Object(reference) => Ok(Self(reference.clone())), _ => unreachable!("validated object value"), } } }
impl From<&Object274> for phenix_core::PhenixValue { fn from(value: &Object274) -> Self { <Object274 as phenix_core::ValueCodec>::to_value(value) } }
impl TryFrom<phenix_core::Exact<&phenix_core::PhenixValue>> for Object274 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Exact<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::from_value(value.0) } }
impl TryFrom<phenix_core::Project<&phenix_core::PhenixValue>> for Object274 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Project<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::project_from_value(value.0) } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural275 { r#CopyOnChange,r#CurrentOnly, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableResource1Type { pub r#binding_path: Vec<String>,pub r#namespace: String,pub r#path: Structural269,pub r#reference: Object274,pub r#resource: String,pub r#schema: phenix_core::PhenixValue,pub r#snapshot_policy: Structural275,pub r#value_id: String,pub r#version: u64, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableResource1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-resource@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeObservableScope1Type { r#Exact,r#Recursive, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableScope1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-scope@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeObservableSnapshotPolicy1Type { r#CopyOnChange,r#CurrentOnly, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableSnapshotPolicy1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-snapshot-policy@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural278 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural279 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural280 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural277 { r#Field(Structural278),r#Index(Structural279),r#MapKey(Structural280),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural276 { pub r#segments: Vec<Structural277>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableSplice1Type { pub r#delete_count: u64,pub r#inserted: Vec<phenix_core::PhenixValue>,pub r#path: Structural276,pub r#start: u64, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableSplice1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-splice@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural281 { r#Full,r#None, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural282 { r#Diff,r#Full, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural285 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural286 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural287 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural284 { r#Field(Structural285),r#Index(Structural286),r#MapKey(Structural287),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural283 { pub r#segments: Vec<Structural284>, }
#[derive(Clone, Debug, PartialEq)]
pub struct Object288(pub phenix_core::ObjectRef);
impl phenix_core::ValueCodec for Object288 { fn phenix_type() -> phenix_core::PhenixSchema { phenix_core::PhenixSchema::Object { contract: phenix_core::ContractId::parse("phenix.observable@1").expect("generated object contract is valid") } } fn to_value(&self) -> phenix_core::PhenixValue { phenix_core::PhenixValue::Object(self.0.clone()) } fn from_value(value: &phenix_core::PhenixValue) -> Result<Self, phenix_core::ValueError> { <Self as phenix_core::ValueCodec>::phenix_type().parse(value)?; match value { phenix_core::PhenixValue::Object(reference) => Ok(Self(reference.clone())), _ => unreachable!("validated object value"), } } }
impl From<&Object288> for phenix_core::PhenixValue { fn from(value: &Object288) -> Self { <Object288 as phenix_core::ValueCodec>::to_value(value) } }
impl TryFrom<phenix_core::Exact<&phenix_core::PhenixValue>> for Object288 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Exact<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::from_value(value.0) } }
impl TryFrom<phenix_core::Project<&phenix_core::PhenixValue>> for Object288 { type Error = phenix_core::ValueError; fn try_from(value: phenix_core::Project<&phenix_core::PhenixValue>) -> Result<Self, Self::Error> { <Self as phenix_core::ValueCodec>::project_from_value(value.0) } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural289 { r#Exact,r#Recursive, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableSubscribeInput1Type { pub r#initial: Structural281,pub r#mode: Structural282,pub r#path: Structural283,pub r#reference: Object288,pub r#scope: Structural289, }
impl phenix_core::PhenixContract for PhenixApplicationTypeObservableSubscribeInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.observable-subscribe-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural294 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural295 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural296 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural293 { r#Field(Structural294),r#Index(Structural295),r#MapKey(Structural296),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural292 { pub r#segments: Vec<Structural293>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural291 { pub r#path: Structural292,pub r#value_id: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural304 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural305 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural306 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural303 { r#Field(Structural304),r#Index(Structural305),r#MapKey(Structural306),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural302 { pub r#segments: Vec<Structural303>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural301 { pub r#path: Structural302, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural300 { pub r#change: Structural301, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural311 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural312 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural313 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural310 { r#Field(Structural311),r#Index(Structural312),r#MapKey(Structural313),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural309 { pub r#segments: Vec<Structural310>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural308 { pub r#path: Structural309,pub r#value: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural307 { pub r#change: Structural308, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural318 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural319 { pub r#index: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural320 { pub r#key: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural317 { r#Field(Structural318),r#Index(Structural319),r#MapKey(Structural320),r#OptionPayload,r#VariantPayload, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural316 { pub r#segments: Vec<Structural317>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural315 { pub r#delete_count: u64,pub r#inserted: Vec<phenix_core::PhenixValue>,pub r#path: Structural316,pub r#start: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural314 { pub r#change: Structural315, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural299 { r#Remove(Structural300),r#Replace(Structural307),r#Splice(Structural314), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural298 { pub r#changes: Vec<Structural299>, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural321 { pub r#value: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural297 { r#Diff(Structural298),r#Full(Structural321), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural290 { pub r#address: Structural291,pub r#commit_id: Option<u64>,pub r#from_version: u64,pub r#generation: u64,pub r#payload: Structural297,pub r#subscription_id: u64,pub r#value_id: String,pub r#version: u64, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeObservableSubscriptionResult1Type { pub r#generation: u64,pub r#initial: Option<Structural290>,pub r#subscription_id: u64, }
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
pub struct Structural323 { pub r#data: phenix_core::Bytes,pub r#mime_type: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural324 { pub r#mime_type: Option<String>,pub r#text: Option<String>,pub r#uri: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural325 { pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural322 { r#Image(Structural323),r#Resource(Structural324),r#Text(Structural325), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypePromptInput1Type { pub r#content: Vec<Structural322>,pub r#session_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypePromptInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.prompt-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural326 { r#Cancelled,r#EndTurn,r#MaxTokens,r#Refused, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypePromptResult1Type { pub r#execution_id: String,pub r#stop_reason: Structural326, }
impl phenix_core::PhenixContract for PhenixApplicationTypePromptResult1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.prompt-result@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeProvenance1Type { pub r#execution_id: String,pub r#inputs: Vec<String>,pub r#model_id: Option<String>,pub r#outputs: Vec<String>,pub r#routing_profile: Option<String>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeProvenance1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.provenance@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeRoutingInfo1Type { pub r#id: String,pub r#name: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeRoutingInfo1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.routing-info@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural327 { pub r#id: String,pub r#name: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeRoutingProfiles1Type { pub r#available: Vec<Structural327>,pub r#selected: Option<String>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeRoutingProfiles1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.routing-profiles@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeRoutingSelectInput1Type { pub r#profile_id: String,pub r#session_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeRoutingSelectInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.routing-select-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSdkValue1Type { pub r#schema: phenix_core::PhenixValue,pub r#value: phenix_core::PhenixValue, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSdkValue1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.sdk-value@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural330 { r#Error,r#Info,r#Warning, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural329 { pub r#code: String,pub r#message: String,pub r#resource: Option<String>,pub r#severity: Structural330, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural328 { pub r#diagnostic: Structural329, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural333 { pub r#fraction: Option<f64>,pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural338 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural339 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural340 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural341 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural342 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural343 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural344 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural345 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural346 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural347 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural348 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural349 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural350 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural351 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural337 { r#Cancelled,r#Closed,r#Conflict(Structural338),r#Disconnected,r#Failed(Structural339),r#InvalidInput(Structural340),r#InvalidPath(Structural341),r#InvalidResponse(Structural342),r#NotFound(Structural343),r#PermissionDenied(Structural344),r#SchemaMismatch(Structural345),r#StaleReference(Structural346),r#SubscriptionCapacity,r#TransactionConflict(Structural347),r#Unauthenticated(Structural348),r#UnknownValue(Structural349),r#UnsupportedCapability(Structural350),r#UnsupportedSnapshotPolicy(Structural351), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural336 { pub r#error: Structural337, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural335 { r#Cancelled,r#Completed,r#Failed(Structural336),r#Pending,r#Running, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural334 { pub r#state: Structural335, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural352 { pub r#call_id: String,pub r#callable_id: String,pub r#input: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural355 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural356 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural357 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural358 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural359 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural360 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural361 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural362 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural363 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural364 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural365 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural366 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural367 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural368 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural354 { r#Cancelled,r#Closed,r#Conflict(Structural355),r#Disconnected,r#Failed(Structural356),r#InvalidInput(Structural357),r#InvalidPath(Structural358),r#InvalidResponse(Structural359),r#NotFound(Structural360),r#PermissionDenied(Structural361),r#SchemaMismatch(Structural362),r#StaleReference(Structural363),r#SubscriptionCapacity,r#TransactionConflict(Structural364),r#Unauthenticated(Structural365),r#UnknownValue(Structural366),r#UnsupportedCapability(Structural367),r#UnsupportedSnapshotPolicy(Structural368), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural353 { pub r#call_id: String,pub r#error: Structural354, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural369 { pub r#call_id: String,pub r#output: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural332 { r#Progress(Structural333),r#State(Structural334),r#ToolCall(Structural352),r#ToolFailed(Structural353),r#ToolResult(Structural369), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural331 { pub r#execution_id: String,pub r#update: Structural332, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural373 { pub r#data: phenix_core::Bytes,pub r#mime_type: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural374 { pub r#mime_type: Option<String>,pub r#text: Option<String>,pub r#uri: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural375 { pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural372 { r#Image(Structural373),r#Resource(Structural374),r#Text(Structural375), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural376 { r#Assistant,r#User, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural371 { pub r#content: Vec<Structural372>,pub r#role: Structural376, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural370 { pub r#message: Structural371, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural377 { pub r#title: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural378 { pub r#execution_id: String,pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum PhenixApplicationTypeSessionChange1Type { r#Closed,r#Diagnostic(Structural328),r#Execution(Structural331),r#Message(Structural370),r#Renamed(Structural377),r#TextDelta(Structural378), }
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
pub struct Structural379 { pub r#session_id: String,pub r#title: Option<String>,pub r#working_directory: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSessionList1Type { pub r#next_cursor: Option<String>,pub r#sessions: Vec<Structural379>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSessionList1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.session-list@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSessionRenameInput1Type { pub r#session_id: String,pub r#title: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSessionRenameInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.session-rename-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSessionResumeInput1Type { pub r#after_sequence: Option<u64>,pub r#session_id: String, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSessionResumeInput1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.session-resume-input@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural380 { pub r#session_id: String,pub r#title: Option<String>,pub r#working_directory: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural385 { r#Error,r#Info,r#Warning, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural384 { pub r#code: String,pub r#message: String,pub r#resource: Option<String>,pub r#severity: Structural385, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural383 { pub r#diagnostic: Structural384, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural388 { pub r#fraction: Option<f64>,pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural393 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural394 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural395 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural396 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural397 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural398 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural399 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural400 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural401 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural402 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural403 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural404 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural405 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural406 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural392 { r#Cancelled,r#Closed,r#Conflict(Structural393),r#Disconnected,r#Failed(Structural394),r#InvalidInput(Structural395),r#InvalidPath(Structural396),r#InvalidResponse(Structural397),r#NotFound(Structural398),r#PermissionDenied(Structural399),r#SchemaMismatch(Structural400),r#StaleReference(Structural401),r#SubscriptionCapacity,r#TransactionConflict(Structural402),r#Unauthenticated(Structural403),r#UnknownValue(Structural404),r#UnsupportedCapability(Structural405),r#UnsupportedSnapshotPolicy(Structural406), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural391 { pub r#error: Structural392, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural390 { r#Cancelled,r#Completed,r#Failed(Structural391),r#Pending,r#Running, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural389 { pub r#state: Structural390, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural407 { pub r#call_id: String,pub r#callable_id: String,pub r#input: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural410 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural411 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural412 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural413 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural414 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural415 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural416 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural417 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural418 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural419 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural420 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural421 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural422 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural423 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural409 { r#Cancelled,r#Closed,r#Conflict(Structural410),r#Disconnected,r#Failed(Structural411),r#InvalidInput(Structural412),r#InvalidPath(Structural413),r#InvalidResponse(Structural414),r#NotFound(Structural415),r#PermissionDenied(Structural416),r#SchemaMismatch(Structural417),r#StaleReference(Structural418),r#SubscriptionCapacity,r#TransactionConflict(Structural419),r#Unauthenticated(Structural420),r#UnknownValue(Structural421),r#UnsupportedCapability(Structural422),r#UnsupportedSnapshotPolicy(Structural423), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural408 { pub r#call_id: String,pub r#error: Structural409, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural424 { pub r#call_id: String,pub r#output: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural387 { r#Progress(Structural388),r#State(Structural389),r#ToolCall(Structural407),r#ToolFailed(Structural408),r#ToolResult(Structural424), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural386 { pub r#execution_id: String,pub r#update: Structural387, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural428 { pub r#data: phenix_core::Bytes,pub r#mime_type: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural429 { pub r#mime_type: Option<String>,pub r#text: Option<String>,pub r#uri: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural430 { pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural427 { r#Image(Structural428),r#Resource(Structural429),r#Text(Structural430), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural431 { r#Assistant,r#User, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural426 { pub r#content: Vec<Structural427>,pub r#role: Structural431, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural425 { pub r#message: Structural426, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural432 { pub r#title: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural433 { pub r#execution_id: String,pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural382 { r#Closed,r#Diagnostic(Structural383),r#Execution(Structural386),r#Message(Structural425),r#Renamed(Structural432),r#TextDelta(Structural433), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural381 { pub r#sequence: u64,pub r#session_id: String,pub r#update: Structural382, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSessionSnapshot1Type { pub r#session: Structural380,pub r#through_sequence: u64,pub r#updates: Vec<Structural381>, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSessionSnapshot1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.session-snapshot@1").expect("generated contract id is valid") } }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural437 { r#Error,r#Info,r#Warning, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural436 { pub r#code: String,pub r#message: String,pub r#resource: Option<String>,pub r#severity: Structural437, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural435 { pub r#diagnostic: Structural436, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural440 { pub r#fraction: Option<f64>,pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural445 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural446 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural447 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural448 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural449 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural450 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural451 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural452 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural453 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural454 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural455 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural456 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural457 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural458 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural444 { r#Cancelled,r#Closed,r#Conflict(Structural445),r#Disconnected,r#Failed(Structural446),r#InvalidInput(Structural447),r#InvalidPath(Structural448),r#InvalidResponse(Structural449),r#NotFound(Structural450),r#PermissionDenied(Structural451),r#SchemaMismatch(Structural452),r#StaleReference(Structural453),r#SubscriptionCapacity,r#TransactionConflict(Structural454),r#Unauthenticated(Structural455),r#UnknownValue(Structural456),r#UnsupportedCapability(Structural457),r#UnsupportedSnapshotPolicy(Structural458), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural443 { pub r#error: Structural444, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural442 { r#Cancelled,r#Completed,r#Failed(Structural443),r#Pending,r#Running, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural441 { pub r#state: Structural442, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural459 { pub r#call_id: String,pub r#callable_id: String,pub r#input: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural462 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural463 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural464 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural465 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural466 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural467 { pub r#resource: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural468 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural469 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural470 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural471 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural472 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural473 { pub r#value: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural474 { pub r#capability: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural475 { pub r#message: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural461 { r#Cancelled,r#Closed,r#Conflict(Structural462),r#Disconnected,r#Failed(Structural463),r#InvalidInput(Structural464),r#InvalidPath(Structural465),r#InvalidResponse(Structural466),r#NotFound(Structural467),r#PermissionDenied(Structural468),r#SchemaMismatch(Structural469),r#StaleReference(Structural470),r#SubscriptionCapacity,r#TransactionConflict(Structural471),r#Unauthenticated(Structural472),r#UnknownValue(Structural473),r#UnsupportedCapability(Structural474),r#UnsupportedSnapshotPolicy(Structural475), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural460 { pub r#call_id: String,pub r#error: Structural461, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural476 { pub r#call_id: String,pub r#output: phenix_core::PhenixValue, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural439 { r#Progress(Structural440),r#State(Structural441),r#ToolCall(Structural459),r#ToolFailed(Structural460),r#ToolResult(Structural476), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural438 { pub r#execution_id: String,pub r#update: Structural439, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural480 { pub r#data: phenix_core::Bytes,pub r#mime_type: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural481 { pub r#mime_type: Option<String>,pub r#text: Option<String>,pub r#uri: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural482 { pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural479 { r#Image(Structural480),r#Resource(Structural481),r#Text(Structural482), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural483 { r#Assistant,r#User, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural478 { pub r#content: Vec<Structural479>,pub r#role: Structural483, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural477 { pub r#message: Structural478, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural484 { pub r#title: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct Structural485 { pub r#execution_id: String,pub r#text: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub enum Structural434 { r#Closed,r#Diagnostic(Structural435),r#Execution(Structural438),r#Message(Structural477),r#Renamed(Structural484),r#TextDelta(Structural485), }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSessionUpdate1Type { pub r#sequence: u64,pub r#session_id: String,pub r#update: Structural434, }
impl phenix_core::PhenixContract for PhenixApplicationTypeSessionUpdate1Type { fn contract_id() -> phenix_core::ContractId { phenix_core::ContractId::parse("phenix.application.type.session-update@1").expect("generated contract id is valid") } }
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
pub struct Structural486 { pub r#active: bool,pub r#description: String,pub r#id: String,pub r#name: String, }
#[derive(Clone, Debug, PartialEq, phenix_sdk_macros::PhenixValue)]
pub struct PhenixApplicationTypeSkills1Type { pub r#skills: Vec<Structural486>, }
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
(<PhenixApplicationTypeCallableInvokeInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeCallableInvokeInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeCallableResult1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeCallableResult1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeCallables1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeCallables1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeCapabilityInvokeInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeCapabilityInvokeInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeCapabilityInvokeResult1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeCapabilityInvokeResult1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
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
(<PhenixApplicationTypeMessageRole1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeMessageRole1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeMessage1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeMessage1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeModelInfo1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeModelInfo1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeModelSelectInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeModelSelectInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeModels1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeModels1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
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
(<PhenixApplicationTypeRoutingInfo1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeRoutingInfo1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeRoutingProfiles1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeRoutingProfiles1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeRoutingSelectInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeRoutingSelectInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSdkValue1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSdkValue1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionChange1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionChange1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionCreateInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionCreateInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionInfo1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionInfo1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionLineage1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionLineage1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionList1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionList1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionRenameInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionRenameInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionResumeInput1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionResumeInput1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionSnapshot1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionSnapshot1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
(<PhenixApplicationTypeSessionUpdate1Type as phenix_core::PhenixContract>::contract_id(), <PhenixApplicationTypeSessionUpdate1Type as phenix_core::HasPhenixSchema>::phenix_schema()),
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
impl phenix_application_interface::Operation for PhenixApplicationCapabilityInvoke1Operation { const ID: &'static str = "phenix.application.capability-invoke@1"; const CAPABILITY: &'static str = "phenix.application.capability.capabilities@1"; type Input = PhenixApplicationTypeCapabilityInvokeInput1Type; type Output = PhenixApplicationTypeCapabilityInvokeResult1Type; }
impl PhenixApplicationCapabilityInvoke1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeCapabilityInvokeInput1Type) -> Result<PhenixApplicationTypeCapabilityInvokeResult1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
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
pub struct PhenixApplicationModelList1Operation;
impl phenix_application_interface::Operation for PhenixApplicationModelList1Operation { const ID: &'static str = "phenix.application.model-list@1"; const CAPABILITY: &'static str = "phenix.application.capability.models@1"; type Input = PhenixApplicationTypeSessionInput1Type; type Output = PhenixApplicationTypeModels1Type; }
impl PhenixApplicationModelList1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeSessionInput1Type) -> Result<PhenixApplicationTypeModels1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationModelSelect1Operation;
impl phenix_application_interface::Operation for PhenixApplicationModelSelect1Operation { const ID: &'static str = "phenix.application.model-select@1"; const CAPABILITY: &'static str = "phenix.application.capability.models@1"; type Input = PhenixApplicationTypeModelSelectInput1Type; type Output = PhenixApplicationTypeModels1Type; }
impl PhenixApplicationModelSelect1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeModelSelectInput1Type) -> Result<PhenixApplicationTypeModels1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
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
pub struct PhenixApplicationRoutingList1Operation;
impl phenix_application_interface::Operation for PhenixApplicationRoutingList1Operation { const ID: &'static str = "phenix.application.routing-list@1"; const CAPABILITY: &'static str = "phenix.application.capability.routing@1"; type Input = PhenixApplicationTypeSessionInput1Type; type Output = PhenixApplicationTypeRoutingProfiles1Type; }
impl PhenixApplicationRoutingList1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeSessionInput1Type) -> Result<PhenixApplicationTypeRoutingProfiles1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationRoutingSelect1Operation;
impl phenix_application_interface::Operation for PhenixApplicationRoutingSelect1Operation { const ID: &'static str = "phenix.application.routing-select@1"; const CAPABILITY: &'static str = "phenix.application.capability.routing@1"; type Input = PhenixApplicationTypeRoutingSelectInput1Type; type Output = PhenixApplicationTypeRoutingProfiles1Type; }
impl PhenixApplicationRoutingSelect1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeRoutingSelectInput1Type) -> Result<PhenixApplicationTypeRoutingProfiles1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
pub struct PhenixApplicationSdkGet1Operation;
impl phenix_application_interface::Operation for PhenixApplicationSdkGet1Operation { const ID: &'static str = "phenix.application.sdk-get@1"; const CAPABILITY: &'static str = "phenix.application.capability.sdk@1"; type Input = PhenixApplicationTypeEmpty1Type; type Output = PhenixApplicationTypeSdkValue1Type; }
impl PhenixApplicationSdkGet1Operation { pub async fn invoke<T: phenix_application_interface::ApplicationTransport>(client: &phenix_application_interface::ApplicationClient<T>, input: PhenixApplicationTypeEmpty1Type) -> Result<PhenixApplicationTypeSdkValue1Type, phenix_application_interface::types::ApplicationError> { client.invoke::<Self>(input).await } }
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
pub type PhenixApplicationCapabilityCall1CallbackRequest = PhenixApplicationTypeCapabilityInvokeInput1Type;
pub const PHENIXAPPLICATIONCAPABILITYCALL1CALLBACKREQUEST: &str = "phenix.application.capability-call@1";
pub type PhenixApplicationCapabilityCall1CallbackResponse = PhenixApplicationTypeCapabilityInvokeResult1Type;
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
pub struct PhenixApplicationCapabilityLineage1Capability; impl PhenixApplicationCapabilityLineage1Capability { pub const ID: &'static str = "phenix.application.capability.lineage@1"; }
pub struct PhenixApplicationCapabilityModels1Capability; impl PhenixApplicationCapabilityModels1Capability { pub const ID: &'static str = "phenix.application.capability.models@1"; }
pub struct PhenixApplicationCapabilityObservables1Capability; impl PhenixApplicationCapabilityObservables1Capability { pub const ID: &'static str = "phenix.application.capability.observables@1"; }
pub struct PhenixApplicationCapabilityPermission1Capability; impl PhenixApplicationCapabilityPermission1Capability { pub const ID: &'static str = "phenix.application.capability.permission@1"; }
pub struct PhenixApplicationCapabilityPrompt1Capability; impl PhenixApplicationCapabilityPrompt1Capability { pub const ID: &'static str = "phenix.application.capability.prompt@1"; }
pub struct PhenixApplicationCapabilityRouting1Capability; impl PhenixApplicationCapabilityRouting1Capability { pub const ID: &'static str = "phenix.application.capability.routing@1"; }
pub struct PhenixApplicationCapabilitySdk1Capability; impl PhenixApplicationCapabilitySdk1Capability { pub const ID: &'static str = "phenix.application.capability.sdk@1"; }
pub struct PhenixApplicationCapabilitySessionList1Capability; impl PhenixApplicationCapabilitySessionList1Capability { pub const ID: &'static str = "phenix.application.capability.session-list@1"; }
pub struct PhenixApplicationCapabilitySessionRename1Capability; impl PhenixApplicationCapabilitySessionRename1Capability { pub const ID: &'static str = "phenix.application.capability.session-rename@1"; }
pub struct PhenixApplicationCapabilitySessionResume1Capability; impl PhenixApplicationCapabilitySessionResume1Capability { pub const ID: &'static str = "phenix.application.capability.session-resume@1"; }
pub struct PhenixApplicationCapabilitySessions1Capability; impl PhenixApplicationCapabilitySessions1Capability { pub const ID: &'static str = "phenix.application.capability.sessions@1"; }
pub struct PhenixApplicationCapabilitySkills1Capability; impl PhenixApplicationCapabilitySkills1Capability { pub const ID: &'static str = "phenix.application.capability.skills@1"; }

use super::*;
use phenix_application_interface::{
    types::{ClientToolAddInput, ClientToolAdmission, ClientToolDefinition, ClientToolRemoveInput},
    AddClientTool, RemoveClientTool,
};
use phenix_core::{CallableId, SessionId};

#[path = "facade.rs"]
mod facade;

fn connect_raw(options: Table) -> LuaResult<Client> {
    super::connect(options)
}

pub(super) fn exports(lua: &Lua) -> LuaResult<Table> {
    let tools = lua.create_table()?;
    tools.set(
        "register",
        lua.create_function(|lua, (definition, handler): (Table, mlua::Function)| {
            let client: mlua::AnyUserData = definition.get("client")?;
            let client = client.borrow::<Client>()?;
            register(lua, &client, definition, handler)
        })?,
    )?;
    Ok(tools)
}

pub(super) fn facade_exports(lua: &Lua) -> LuaResult<Table> {
    let application = lua.create_table()?;
    application.set(
        "connect",
        lua.create_function(|lua, options: Table| facade::connect(lua, options))?,
    )?;
    Ok(application)
}

pub(super) fn bind(lua: &Lua, client: Client) -> LuaResult<Table> {
    let tools = lua.create_table()?;
    tools.set(
        "register",
        lua.create_function(move |lua, (definition, handler): (Table, mlua::Function)| {
            register(lua, &client, definition, handler)
        })?,
    )?;
    Ok(tools)
}

fn operation(client: &Client, id: &str) -> LuaResult<ContractId> {
    let operation =
        ContractId::parse(id).map_err(|error| lua_error(BindingError::conversion(error)))?;
    if !client
        .state
        .supports_extension(&operation)
        .map_err(lua_error)?
    {
        return Err(lua_error(BindingError::unsupported(&operation)));
    }
    Ok(operation)
}

fn register(
    lua: &Lua,
    client: &Client,
    definition: Table,
    handler: mlua::Function,
) -> LuaResult<Request> {
    let operation = operation(client, AddClientTool::ID)?;
    // Validate all metadata before retaining the host function.
    for pair in definition.clone().pairs::<String, Value>() {
        let (key, _) = pair?;
        if !matches!(
            key.as_str(),
            "client"
                | "session_id"
                | "id"
                | "description"
                | "input"
                | "output"
                | "capabilities"
                | "requires_permission"
        ) {
            return Err(lua_error(BindingError::conversion(format!(
                "unexpected tool definition field {key}"
            ))));
        }
    }
    let session_id = SessionId::parse(definition.get::<String>("session_id")?)
        .map_err(|error| lua_error(BindingError::conversion(error)))?;
    let id = CallableId::parse(definition.get::<String>("id")?)
        .map_err(|error| lua_error(BindingError::conversion(error)))?;
    let description: String = definition.get("description")?;
    if description.trim().is_empty() {
        return Err(lua_error(BindingError::conversion(
            "tool description must not be empty",
        )));
    }
    let input: Type = lua.from_value(definition.get("input")?)?;
    let output: Type = lua.from_value(definition.get("output")?)?;
    let capabilities = definition
        .get::<Option<Vec<String>>>("capabilities")?
        .unwrap_or_default();
    let requires_permission = definition
        .get::<Option<bool>>("requires_permission")?
        .unwrap_or(false);
    let schema = Type::Callable {
        contract: ContractId::parse("phenix.client-tool-handler@1").expect("static contract id"),
        input: Box::new(input.clone()),
        output: Box::new(output.clone()),
    };
    let invoke = lua_to_phenix_with_host(
        lua,
        &schema,
        Value::Function(handler),
        &client.state,
        &client.local_callables,
    )
    .map_err(lua_error)?;
    let PhenixValue::Callable(reference) = &invoke else {
        return Err(lua_error(BindingError::conversion(
            "function lifting must return a callable",
        )));
    };
    let reference = reference.id().clone();
    let input = ClientToolAddInput {
        session_id: session_id.clone(),
        tool: ClientToolDefinition {
            id,
            description,
            input,
            output,
            capabilities,
            requires_permission,
            invoke,
        },
    }
    .to_value();
    let request = request_for(
        &client.state,
        Some(Rc::clone(&client.local_callables)),
        |reply| Command::Application {
            operation,
            input,
            reply,
        },
    );
    match request {
        Ok(mut request) => {
            request.tool_registration = Some(Registration {
                client: client.clone(),
                session_id,
                reference,
                stop: None,
            });
            Ok(request)
        }
        Err(error) => {
            client
                .local_callables
                .borrow_mut()
                .entries
                .remove(&reference);
            Err(error)
        }
    }
}

pub(super) struct Registration {
    client: Client,
    session_id: SessionId,
    reference: ReferenceId,
    stop: Option<mlua::Function>,
}

impl Registration {
    pub(super) fn release(&self) {
        self.client
            .local_callables
            .borrow_mut()
            .entries
            .remove(&self.reference);
    }

    pub(super) fn project(&mut self, lua: &Lua, response: Response) -> LuaResult<MultiValue> {
        if let Some(stop) = &self.stop {
            return Ok(MultiValue::from_vec(vec![Value::Function(stop.clone())]));
        }
        let Response::Application { value, .. } = response else {
            return Err(lua_error(BindingError::conversion(
                "expected tool admission response",
            )));
        };
        let admission = ClientToolAdmission::from_value(&value)
            .map_err(|error| lua_error(BindingError::conversion(error.to_string())))?;
        let client = self.client.clone();
        let session_id = self.session_id.clone();
        let reference = self.reference.clone();
        // Repeated local stop calls share one request, including its failure.
        // Explicit application removal still preserves the remote stale error.
        let pending: Rc<RefCell<Option<mlua::AnyUserData>>> = Rc::new(RefCell::new(None));
        let stop = lua.create_function(move |lua, ()| {
            if let Some(request) = pending.borrow().as_ref() {
                return Ok(request.clone());
            }
            let operation = operation(&client, RemoveClientTool::ID)?;
            let input = ClientToolRemoveInput {
                session_id: session_id.clone(),
                admission_id: admission.admission_id.clone(),
            }
            .to_value();
            let request = request_for(
                &client.state,
                Some(Rc::clone(&client.local_callables)),
                |reply| Command::Application {
                    operation,
                    input,
                    reply,
                },
            )?
            .with_listener_lifecycle(ListenerLifecycle::RemoveOnSuccess(reference.clone()));
            let request = lua.create_userdata(request)?;
            *pending.borrow_mut() = Some(request.clone());
            Ok(request)
        })?;
        self.stop = Some(stop.clone());
        Ok(MultiValue::from_vec(vec![Value::Function(stop)]))
    }
}

#[cfg(test)]
#[path = "tools_regression.rs"]
mod tests;

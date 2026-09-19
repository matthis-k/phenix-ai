use super::*;

fn client() -> (Client, mpsc::UnboundedReceiver<Command>) {
    let (commands, receiver) = mpsc::unbounded();
    let (_, updates) = std_mpsc::channel();
    let (_, extension_updates) = std_mpsc::channel();
    let (_, callbacks) = std_mpsc::channel();
    let state = Arc::new(ClientState {
        commands,
        updates: Mutex::new(updates),
        extension_updates: Mutex::new(extension_updates),
        callbacks: Mutex::new(callbacks),
        capabilities: Mutex::new(BTreeSet::new()),
        extensions: Mutex::new(BTreeSet::from([
            AddClientTool::ID.to_owned(),
            RemoveClientTool::ID.to_owned(),
        ])),
        session_config_options: Mutex::new(BTreeMap::new()),
        terminal_error: Mutex::new(None),
        owner: ClientConnectionId::parse("fixture-client").unwrap(),
        generation: CapabilityGenerationId::parse("generation-1").unwrap(),
    });
    (
        Client {
            state,
            local_callables: Rc::new(RefCell::new(LocalCallables::default())),
        },
        receiver,
    )
}

fn definition(lua: &Lua) -> Table {
    let definition = lua.create_table().unwrap();
    definition.set("session_id", "session-a").unwrap();
    definition.set("id", "fixture.echo").unwrap();
    definition.set("description", "Echo a number").unwrap();
    definition
        .set("input", lua.to_value(&Type::U64).unwrap())
        .unwrap();
    definition
        .set("output", lua.to_value(&Type::U64).unwrap())
        .unwrap();
    definition
}

#[test]
fn registration_lifts_exact_schema_and_stop_is_idempotent() {
    let lua = Lua::new();
    let (client, mut commands) = client();
    let definition = definition(&lua);
    definition
        .set("client", lua.create_userdata(client.clone()).unwrap())
        .unwrap();
    let register: mlua::Function = exports(&lua).unwrap().get("register").unwrap();
    let handler = lua
        .load("return function(value) return value end")
        .eval::<mlua::Function>()
        .unwrap();
    let request: mlua::AnyUserData = register.call((definition, handler)).unwrap();
    let Command::Application {
        operation,
        input,
        reply,
    } = futures::executor::block_on(commands.next()).unwrap()
    else {
        panic!("registration must use ordinary application admission");
    };
    assert_eq!(operation.as_str(), AddClientTool::ID);
    let input = ClientToolAddInput::from_value(&input).unwrap();
    let PhenixValue::Callable(reference) = input.tool.invoke else {
        panic!("tool behavior must be a lifted capability");
    };
    assert_eq!(input.tool.input, Type::U64);
    assert_eq!(input.tool.output, Type::U64);
    assert_eq!(
        client.local_callables.borrow().entries[reference.id()].schema,
        Type::Callable {
            contract: reference.contract().clone(),
            input: Box::new(Type::U64),
            output: Box::new(Type::U64),
        }
    );
    reply
        .send(Ok(Response::Application {
            operation,
            value: ClientToolAdmission {
                admission_id: "admission-1".to_owned(),
                callable_id: input.tool.id,
            }
            .to_value(),
        }))
        .unwrap();
    lua.globals().set("registration", request).unwrap();
    let stop: mlua::Function = lua.load("return registration:poll()").eval().unwrap();
    let stop_again: mlua::Function = lua.load("return registration:poll()").eval().unwrap();
    let removal: mlua::AnyUserData = stop.call(()).unwrap();
    let removal_again: mlua::AnyUserData = stop_again.call(()).unwrap();
    assert_eq!(removal, removal_again);
    let Command::Application {
        operation,
        input,
        reply,
    } = futures::executor::block_on(commands.next()).unwrap()
    else {
        panic!("stop must use ordinary removal");
    };
    assert_eq!(operation.as_str(), RemoveClientTool::ID);
    assert_eq!(
        ClientToolRemoveInput::from_value(&input)
            .unwrap()
            .admission_id,
        "admission-1"
    );
    assert!(client
        .local_callables
        .borrow()
        .entries
        .contains_key(reference.id()));
    reply.send(Ok(Response::Acknowledged)).unwrap();
    lua.globals().set("removal", removal).unwrap();
    lua.load("return removal:poll()").eval::<Value>().unwrap();
    assert!(!client
        .local_callables
        .borrow()
        .entries
        .contains_key(reference.id()));
    assert!(commands.try_recv().is_err());
}

#[test]
fn rejected_admission_releases_lifted_handler() {
    let lua = Lua::new();
    let (client, mut commands) = client();
    let register: mlua::Function = bind(&lua, client.clone()).unwrap().get("register").unwrap();
    let handler = lua
        .load("return function(value) return value end")
        .eval::<mlua::Function>()
        .unwrap();
    let request: mlua::AnyUserData = register.call((definition(&lua), handler)).unwrap();
    assert_eq!(client.local_callables.borrow().entries.len(), 1);
    let Command::Application { reply, .. } = futures::executor::block_on(commands.next()).unwrap()
    else {
        panic!("registration must send admission");
    };
    reply
        .send(Err(BindingError::local(
            ErrorKind::Rejected,
            "duplicate id",
        )))
        .unwrap();
    lua.globals().set("registration", request).unwrap();
    let (result, error): (Value, Table) = lua.load("return registration:poll()").eval().unwrap();
    assert!(matches!(result, Value::Nil));
    assert_eq!(error.get::<String>("kind").unwrap(), "rejected");
    assert!(client.local_callables.borrow().entries.is_empty());
}

#[test]
fn malformed_metadata_does_not_retain_a_handler() {
    let lua = Lua::new();
    let (client, mut commands) = client();
    let register: mlua::Function = bind(&lua, client.clone()).unwrap().get("register").unwrap();
    let definition = definition(&lua);
    definition.set("unexpected", true).unwrap();
    let handler = lua
        .load("return function(value) return value end")
        .eval::<mlua::Function>()
        .unwrap();
    assert!(register.call::<Value>((definition, handler)).is_err());
    assert!(client.local_callables.borrow().entries.is_empty());
    assert!(commands.try_recv().is_err());
}

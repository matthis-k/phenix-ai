// Explicit field conversion; no serialized Message hidden inside a string port.
use dataflow_fixture as common;
use dataflow_native_contracts as native;

pub fn native_identity(id: common::Identity) -> native::Identity {
    native::Identity {
        root: id.root,
        wave: id.wave,
        activation: id.activation,
        occurrence: id.occurrence,
    }
}

pub fn common_identity(id: native::Identity) -> common::Identity {
    common::Identity {
        root: id.root,
        wave: id.wave,
        activation: id.activation,
        occurrence: id.occurrence,
    }
}

pub fn native_message(m: &common::Message) -> native::Message {
    native::Message {
        id: native_identity(m.id),
        text: m.text.clone(),
        tool: m.tool,
        pending: m.pending,
    }
}

pub fn common_message(m: native::Message) -> common::Message {
    common::Message {
        id: common_identity(m.id),
        text: m.text,
        tool: m.tool,
        pending: m.pending,
    }
}

pub fn native_branch(m: common::Message) -> native::Branch {
    let native::Message {
        id,
        text,
        tool,
        pending,
    } = native_message(&m);
    if tool {
        native::Branch::Tool {
            id,
            text,
            tool,
            pending,
        }
    } else {
        native::Branch::Model {
            id,
            text,
            tool,
            pending,
        }
    }
}

pub fn native_result(m: common::Message) -> native::ResultValue {
    native::ResultValue {
        id: native_identity(m.id),
        text: common::response(&m, ""),
    }
}

pub fn common_result(r: native::ResultValue) -> common::ResultValue {
    common::ResultValue {
        id: common_identity(r.id),
        text: r.text,
    }
}

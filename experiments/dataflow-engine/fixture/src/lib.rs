use serde::{Deserialize, Serialize};
use std::{cell::RefCell, rc::Rc};
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct Identity {
    pub root: u64,
    pub wave: u64,
    pub activation: u64,
    pub occurrence: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: Identity,
    pub text: String,
    pub tool: bool,
    pub pending: bool,
}
#[derive(Clone, Debug)]
pub struct Prepared(pub Message);
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResultValue {
    pub id: Identity,
    pub text: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Event {
    pub id: Identity,
    pub node: &'static str,
}
pub type Trace = Rc<RefCell<Vec<Event>>>;
pub fn trace(t: &Trace, id: Identity, node: &'static str) {
    t.borrow_mut().push(Event { id, node });
}
pub fn prepare(mut m: Message, t: &Trace) -> Prepared {
    trace(t, m.id, "Prepare");
    m.text = m.text.trim().to_uppercase();
    Prepared(m)
}
pub fn switch(p: Prepared, t: &Trace) -> Prepared {
    trace(t, p.0.id, "Switch");
    p
}
pub fn aggregate(v: ResultValue, t: &Trace) -> ResultValue {
    trace(t, v.id, "Aggregate");
    v
}
pub fn complete(v: ResultValue, t: &Trace) -> ResultValue {
    trace(t, v.id, "Complete");
    v
}

pub fn response(m: &Message, bias: &str) -> String {
    format!(
        "{}:{}{}",
        if m.tool { "tool" } else { "model" },
        m.text,
        bias
    )
}
pub fn message(root: u64, tool: bool, pending: bool) -> Message {
    Message {
        id: Identity {
            root,
            wave: 0,
            activation: root,
            occurrence: 0,
        },
        text: " hello ".into(),
        tool,
        pending,
    }
}

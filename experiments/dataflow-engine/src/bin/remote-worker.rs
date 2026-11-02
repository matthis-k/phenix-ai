use dataflow_fixture::{response, Message};
use std::io::{Read, Write};
fn main() {
    let mut wire = String::new();
    std::io::stdin().read_to_string(&mut wire).unwrap();
    let m: Message = serde_json::from_str(&wire).unwrap();
    match m.text.as_str() {
        "EXIT" => std::process::exit(23),
        "HANG" => {
            std::thread::park();
        }
        _ => {}
    }
    let out = serde_json::to_vec(&(m.id, response(&m, ""))).unwrap();
    std::io::stdout().write_all(&out).unwrap();
}

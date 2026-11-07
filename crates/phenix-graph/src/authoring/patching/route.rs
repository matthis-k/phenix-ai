//! Conservative symbolic write footprints for pre-compilation patches.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PortSelector {
    pub node: String,
    pub port: Option<String>,
    pub slot: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectionRoute {
    pub from: PortSelector,
    pub to: PortSelector,
}
impl PortSelector {
    pub(super) fn parse(path: &str, direction: &str, is_input: bool) -> Result<PortSelector, PatchError> {
        if path.is_empty() { return Err(PatchError::InvalidEndpoint(path.into())); }
        let qualified = format!(".{direction}.");
        let shorthand = format!(".{direction}");
        let (node, suffix) = if let Some((node, suffix)) = path.split_once(&qualified) {
            (node, Some(suffix))
        } else if let Some(node) = path.strip_suffix(&shorthand) {
            (node, None)
        } else if path.contains('.') {
            return Err(PatchError::InvalidEndpoint(path.into()));
        } else { (path, None) };
        if node.is_empty() || node.contains('.') || node.chars().any(char::is_control) ||
            suffix.is_some_and(str::is_empty) {
            return Err(PatchError::InvalidEndpoint(path.into()));
        }
        let (port, slot) = match suffix {
            Some(rest) if is_input => {
                if let Some((port, slot)) = rest.split_once('.') {
                    if port.is_empty() || slot.is_empty() { return Err(PatchError::InvalidEndpoint(path.into())); }
                    (Some(port.into()), Some(slot.into()))
                } else { (Some(rest.into()), None) }
            }
            Some(rest) if rest.contains('.') => return Err(PatchError::InvalidEndpoint(path.into())),
            Some(rest) => (Some(rest.into()), None),
            None => (None, None),
        };
        Ok(PortSelector { node: node.into(), port, slot })
    }
}
impl ConnectionRoute {
    pub fn parse(from: &str, to: &str) -> Result<Self, PatchError> {
        Ok(Self { from: PortSelector::parse(from, "out", false)?,
            to: PortSelector::parse(to, "in", true)? })
    }
    pub(super) fn pattern_overlaps(&self, other: &Self) -> bool {
        fn compatible(a: &Option<String>, b: &Option<String>) -> bool {
            a.is_none() || b.is_none() || a == b
        }
        self.from.node == other.from.node && self.to.node == other.to.node &&
            compatible(&self.from.port, &other.from.port) &&
            compatible(&self.to.port, &other.to.port) &&
            compatible(&self.to.slot, &other.to.slot)
    }
    pub(super) fn from_exact_key(key: &str) -> Option<Self> {
        // Parse length-prefixed fields by byte offsets. Splitting on `->`
        // would be incorrect because that sequence may occur inside a name.
        fn head(input: &str) -> Option<(bool, &str)> {
            if let Some(rest) = input.strip_prefix("node/") { Some((true, rest)) }
            else { input.strip_prefix("instance/").map(|rest| (false, rest)) }
        }
        fn field(input: &str) -> Option<(String, &str)> {
            let colon = input.find(':')?;
            let length = input[..colon].parse::<usize>().ok()?;
            let rest = input.get(colon+1..)?;
            let value = rest.get(..length)?;
            let remaining = rest.get(length..)?;
            Some((value.to_owned(), remaining))
        }
        let (_, src) = head(key)?;
        let (source_name, rest) = field(src)?;
        let (source_port, rest) = field(rest.strip_prefix('/')?)?;
        let (dst_node, rest) = head(rest.strip_prefix("->")?)?;
        let (target_name, rest) = field(rest)?;
        let (target_port, rest) = field(rest.strip_prefix('/')?)?;
        let target_slot = if dst_node {
            let (slot, rest) = field(rest.strip_prefix('/')?)?;
            if !rest.is_empty() { return None; }
            Some(slot)
        } else {
            if !rest.is_empty() { return None; }
            None
        };
        Some(Self {
            from: PortSelector { node: source_name, port: Some(source_port), slot: None },
            to: PortSelector { node: target_name, port: Some(target_port), slot: target_slot },
        })
    }
}

from pathlib import Path
import re


def move_request_continuation() -> None:
    path = Path('rust/crates/phenix-core/src/agent.rs')
    text = path.read_text()
    wrong = '''    /// Completed prior assistant/tool turns for this logical model turn.\n    #[serde(default)]\n    pub continuation: Vec<ModelToolTurn>,\n'''
    if wrong in text:
        text = text.replace(wrong, '', 1)
    request_tail = '''    /// The complete model-visible tool surface for this inference turn.\n    #[serde(default)]\n    pub tools: Vec<ModelToolDescriptor>,\n}\n\n#[derive(phenix_sdk_macros::PhenixValue, Clone, Debug, PartialEq, Serialize, Deserialize)]\npub struct ModelInferenceResponse'''
    replacement = '''    /// The complete model-visible tool surface for this inference turn.\n    #[serde(default)]\n    pub tools: Vec<ModelToolDescriptor>,\n    /// Completed assistant/tool exchanges for this logical inference turn.\n    #[serde(default)]\n    pub continuation: Vec<ModelToolTurn>,\n}\n\n#[derive(phenix_sdk_macros::PhenixValue, Clone, Debug, PartialEq, Serialize, Deserialize)]\npub struct ModelInferenceResponse'''
    if 'pub continuation: Vec<ModelToolTurn>' not in text.split('pub struct ModelInferenceResponse', 1)[0]:
        if request_tail not in text:
            raise SystemExit('ModelInferenceRequest source shape changed')
        text = text.replace(request_tail, replacement, 1)
    path.write_text(text)


def matching_brace(text: str, open_at: int) -> int:
    depth = 0
    i = open_at
    quote = None
    escaped = False
    line_comment = False
    block_comment = 0
    while i < len(text):
        c = text[i]
        n = text[i + 1] if i + 1 < len(text) else ''
        if line_comment:
            if c == '\n':
                line_comment = False
            i += 1
            continue
        if block_comment:
            if c == '/' and n == '*':
                block_comment += 1
                i += 2
                continue
            if c == '*' and n == '/':
                block_comment -= 1
                i += 2
                continue
            i += 1
            continue
        if quote:
            if escaped:
                escaped = False
            elif c == '\\':
                escaped = True
            elif c == quote:
                quote = None
            i += 1
            continue
        if c == '/' and n == '/':
            line_comment = True
            i += 2
            continue
        if c == '/' and n == '*':
            block_comment = 1
            i += 2
            continue
        if c in ('"', "'"):
            quote = c
            i += 1
            continue
        if c == '{':
            depth += 1
        elif c == '}':
            depth -= 1
            if depth == 0:
                return i
        i += 1
    raise SystemExit('unbalanced Rust braces while migrating continuation literals')


def line_brace_delta(line: str) -> int:
    depth = 0
    quote = None
    escaped = False
    i = 0
    while i < len(line):
        c = line[i]
        n = line[i + 1] if i + 1 < len(line) else ''
        if quote:
            if escaped:
                escaped = False
            elif c == '\\':
                escaped = True
            elif c == quote:
                quote = None
            i += 1
            continue
        if c == '/' and n == '/':
            break
        if c in ('"', "'"):
            quote = c
        elif c == '{':
            depth += 1
        elif c == '}':
            depth -= 1
        i += 1
    return depth


def top_level_field_indent(body: str, name: str):
    depth = 0
    pattern = re.compile(rf'^(\s*){re.escape(name)}\s*:')
    for line in body.splitlines(keepends=True):
        match = pattern.match(line) if depth == 0 else None
        if match:
            return match.group(1)
        depth += line_brace_delta(line)
    return None


def insert_for_marker(text: str, marker: str) -> tuple[str, int]:
    pos = 0
    changed = 0
    needle = marker + ' {'
    while True:
        start = text.find(needle, pos)
        if start < 0:
            return text, changed
        prefix = text[max(0, start - 32):start]
        if re.search(r'\b(?:struct|enum)\s+$', prefix):
            pos = start + len(needle)
            continue
        open_at = start + len(needle) - 1
        close_at = matching_brace(text, open_at)
        body = text[open_at + 1:close_at]
        tools_indent = top_level_field_indent(body, 'tools')
        has_continuation = top_level_field_indent(body, 'continuation') is not None
        if tools_indent is None or has_continuation:
            pos = close_at + 1
            continue
        stripped = body.rstrip()
        suffix_ws = body[len(stripped):]
        if stripped and not stripped.endswith(','):
            stripped += ','
        insertion = f'\n{tools_indent}continuation: Vec::new(),'
        body = stripped + insertion + suffix_ws
        text = text[:open_at + 1] + body + text[close_at:]
        changed += 1
        pos = open_at + 1 + len(body) + 1


move_request_continuation()
markers = (
    'ModelInferenceRequest',
    'InvocationRequest',
    'PlannedStepRequest',
    'ModelDispatchCommand::PrepareResolved',
    'AgentLoopCommand::Run',
)
counts = {marker: 0 for marker in markers}
for path in Path('rust').rglob('*.rs'):
    text = path.read_text()
    original = text
    for marker in markers:
        text, count = insert_for_marker(text, marker)
        counts[marker] += count
    if text != original:
        path.write_text(text)

for marker in ('ModelInferenceRequest', 'InvocationRequest', 'PlannedStepRequest'):
    if counts[marker] == 0:
        raise SystemExit(f'no {marker} literals were migrated; source shape likely changed')

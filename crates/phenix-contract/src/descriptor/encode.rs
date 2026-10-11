use super::{Declaration, ImportPolicy, Schema};

// Constant evaluation is the shared lowering mechanism. The macro supplies typed
// declarations; it never reads source files or maintains a metadata registry.
pub const SECTION_BYTES: usize = 16 * 1024;
struct Writer {
    bytes: [u8; SECTION_BYTES],
    at: usize,
}
impl Writer {
    const fn byte(&mut self, b: u8) {
        assert!(self.at < SECTION_BYTES, "descriptor exceeds 16 KiB");
        self.bytes[self.at] = b;
        self.at += 1;
    }
    const fn raw(&mut self, s: &str) {
        let b = s.as_bytes();
        let mut i = 0;
        while i < b.len() {
            self.byte(b[i]);
            i += 1;
        }
    }
    const fn n(&mut self, n: usize) {
        let b = (n as u32).to_be_bytes();
        let mut i = 0;
        while i < 4 {
            self.byte(b[i]);
            i += 1;
        }
    }
    const fn key(&mut self, s: &str) {
        self.n(s.len());
        self.raw(s);
    }
    const fn text(&mut self, s: &str) {
        self.byte(5);
        self.key(s);
    }
    const fn u64(&mut self, n: u64) {
        self.byte(4);
        let b = n.to_be_bytes();
        let mut i = 0;
        while i < 8 {
            self.byte(b[i]);
            i += 1;
        }
    }
    const fn map(&mut self, n: usize) {
        self.byte(8);
        self.n(n);
    }
    const fn list(&mut self, n: usize) {
        self.byte(7);
        self.n(n);
    }
    const fn schema(&mut self, s: Schema) {
        match s {
            Schema::Struct(fields) => {
                self.map(2);
                self.key("fields");
                self.map(fields.len());
                let mut i = 0;
                while i < fields.len() {
                    if i > 0 {
                        assert!(
                            compare(fields[i - 1].name, fields[i].name) < 0,
                            "schema fields must be sorted"
                        );
                    }
                    self.key(fields[i].name);
                    self.schema(fields[i].schema);
                    i += 1;
                }
                self.key("kind");
                self.text("Struct");
            }
            Schema::Variants(variants) => {
                self.map(2);
                self.key("kind");
                self.text("Variants");
                self.key("variants");
                self.list(variants.len());
                let mut i = 0;
                while i < variants.len() {
                    self.text(variants[i]);
                    i += 1;
                }
            }
            other => {
                self.map(1);
                self.key("kind");
                self.text(match other {
                    Schema::Bool => "Bool",
                    Schema::I64 => "I64",
                    Schema::U64 => "U64",
                    Schema::String => "String",
                    Schema::Bytes => "Bytes",
                    _ => panic!("exhaustive schema"),
                });
            }
        }
    }
    const fn contribution(
        &mut self,
        owner: &str,
        d: Declaration,
        role: &str,
        policy: ImportPolicy,
    ) {
        let enhanced = policy.optional || policy.implementation.is_some();
        self.map(if enhanced { 8 } else { 6 });
        self.key("id");
        self.byte(5);
        self.n(owner.len() + d.operation.key.len() + role.len() + 2);
        self.raw(owner);
        self.raw("/");
        self.raw(d.operation.key);
        self.raw("/");
        self.raw(role);
        if enhanced {
            self.key("implementation");
            match policy.implementation {
                Some(id) => self.text(id),
                None => self.byte(0),
            }
        }
        self.key("kind");
        self.text(if enhanced {
            "phenix.import.v1"
        } else {
            "phenix.operation.v1"
        });
        self.key("operation");
        self.text(d.operation.key);
        if enhanced {
            self.key("optional");
            self.byte(if policy.optional { 2 } else { 1 });
        }
        self.key("owner");
        self.text(owner);
        self.key("provenance");
        self.text(d.provenance);
        self.key("role");
        self.text(role);
    }
}
const fn compare(a: &str, b: &str) -> i32 {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut i = 0;
    while i < a.len() && i < b.len() {
        if a[i] < b[i] {
            return -1;
        }
        if a[i] > b[i] {
            return 1;
        }
        i += 1;
    }
    if a.len() < b.len() {
        -1
    } else if a.len() > b.len() {
        1
    } else {
        0
    }
}
const fn equal_schema(a: Schema, b: Schema) -> bool {
    match (a, b) {
        (Schema::Bool, Schema::Bool)
        | (Schema::I64, Schema::I64)
        | (Schema::U64, Schema::U64)
        | (Schema::String, Schema::String)
        | (Schema::Bytes, Schema::Bytes) => true,
        (Schema::Struct(a), Schema::Struct(b)) => {
            if a.len() != b.len() {
                return false;
            }
            let mut i = 0;
            while i < a.len() {
                if compare(a[i].name, b[i].name) != 0 || !equal_schema(a[i].schema, b[i].schema) {
                    return false;
                }
                i += 1;
            }
            true
        }
        (Schema::Variants(a), Schema::Variants(b)) => {
            if a.len() != b.len() {
                return false;
            }
            let mut i = 0;
            while i < a.len() {
                if compare(a[i], b[i]) != 0 {
                    return false;
                }
                i += 1;
            }
            true
        }
        _ => false,
    }
}
pub const fn descriptor<const I: usize, const E: usize>(
    id: &str,
    version: &str,
    imports: [Declaration; I],
    exports: [Declaration; E],
) -> [u8; SECTION_BYTES] {
    descriptor_with_policy(id, version, imports, [ImportPolicy::REQUIRED; I], exports)
}
pub const fn descriptor_with_policy<const I: usize, const E: usize>(
    id: &str,
    version: &str,
    mut imports: [Declaration; I],
    mut policies: [ImportPolicy; I],
    mut exports: [Declaration; E],
) -> [u8; SECTION_BYTES] {
    let mut i = 1;
    while i < I {
        let mut j = i;
        while j > 0 && compare(imports[j - 1].operation.key, imports[j].operation.key) > 0 {
            let d = imports[j];
            imports[j] = imports[j - 1];
            imports[j - 1] = d;
            let p = policies[j];
            policies[j] = policies[j - 1];
            policies[j - 1] = p;
            j -= 1;
        }
        i += 1;
    }
    let mut i = 1;
    while i < E {
        let mut j = i;
        while j > 0 && compare(exports[j - 1].operation.key, exports[j].operation.key) > 0 {
            let d = exports[j];
            exports[j] = exports[j - 1];
            exports[j - 1] = d;
            j -= 1;
        }
        i += 1;
    }
    let mut i = 1;
    while i < I {
        assert!(
            compare(imports[i - 1].operation.key, imports[i].operation.key) != 0,
            "duplicate import"
        );
        i += 1;
    }
    let mut i = 1;
    while i < E {
        assert!(
            compare(exports[i - 1].operation.key, exports[i].operation.key) != 0,
            "duplicate export"
        );
        i += 1;
    }
    let mut w = Writer {
        bytes: [0; SECTION_BYTES],
        at: 4,
    };
    w.map(12);
    w.key("abi_major");
    w.u64(1);
    w.key("abi_minor");
    w.u64(0);

    let mut count = I + E;
    let mut i = 0;
    while i < I {
        let mut j = 0;
        while j < E {
            if compare(imports[i].operation.key, exports[j].operation.key) == 0 {
                assert!(
                    equal_schema(imports[i].operation.request, exports[j].operation.request)
                        && equal_schema(
                            imports[i].operation.response,
                            exports[j].operation.response
                        )
                        && equal_schema(imports[i].operation.error, exports[j].operation.error),
                    "conflicting contract schema"
                );
                count -= 1;
            }
            j += 1;
        }
        i += 1;
    }
    w.key("authority_ceiling");
    w.list(count);
    let (mut i, mut j) = (0, 0);
    while i < I || j < E {
        let op = if j == E
            || (i < I && compare(imports[i].operation.key, exports[j].operation.key) < 0)
        {
            let op = imports[i].operation;
            i += 1;
            op
        } else {
            let op = exports[j].operation;
            j += 1;
            if i < I && compare(imports[i].operation.key, op.key) == 0 {
                i += 1;
            }
            op
        };
        w.text(op.key);
    }
    w.key("contracts");
    w.list(count);
    let (mut i, mut j) = (0, 0);
    while i < I || j < E {
        let op = if j == E
            || (i < I && compare(imports[i].operation.key, exports[j].operation.key) < 0)
        {
            let op = imports[i].operation;
            i += 1;
            op
        } else {
            let op = exports[j].operation;
            j += 1;
            if i < I && compare(imports[i].operation.key, op.key) == 0 {
                i += 1;
            }
            op
        };
        w.map(9);
        w.key("error");
        w.schema(op.error);
        w.key("id");
        w.byte(5);
        w.n(id.len() + op.key.len() + 9);
        w.raw(id);
        w.raw("/");
        w.raw(op.key);
        w.raw("/Declare");
        w.key("key");
        w.text(op.key);
        w.key("kind");
        w.text("phenix.contract.v1");
        w.key("owner");
        w.text(id);
        w.key("provenance");
        w.text(op.key);
        w.key("request");
        w.schema(op.request);
        w.key("response");
        w.schema(op.response);
        w.key("role");
        w.text("Declare");
    }
    w.key("execution");
    w.text("sync");
    w.key("exports");
    w.list(E);
    let mut i = 0;
    while i < E {
        w.contribution(id, exports[i], "Provide", ImportPolicy::REQUIRED);
        i += 1;
    }
    w.key("features");
    w.u64(0);
    w.key("format");
    w.text("phenix.descriptor.v1");
    w.key("imports");
    w.list(I);
    let mut i = 0;
    while i < I {
        w.contribution(id, imports[i], "Require", policies[i]);
        i += 1;
    }
    w.key("plugin_id");
    w.text(id);
    w.key("plugin_version");
    w.text(version);
    w.key("schema_version");
    w.u64(1);
    let n = ((w.at - 4) as u32).to_be_bytes();
    let mut i = 0;
    while i < 4 {
        w.bytes[i] = n[i];
        i += 1;
    }
    w.bytes
}

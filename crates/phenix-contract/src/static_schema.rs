//! Constant structural schemas for independently compiled M1 API crates.
//! Encoding and hashing are byte-identical to ValueSchema, never Rust TypeId.
use crate::{Failure, SchemaId, ValueSchema};
#[derive(Clone, Copy, Debug)]
pub enum StaticSchema {
    Bool,
    I64,
    U64,
    String,
    Bytes,
    List(&'static StaticSchema),
    Option(&'static StaticSchema),
    Struct(&'static [StaticField]),
    Variants(&'static [&'static str]),
    Tagged(&'static [StaticVariant]),
    Ref(&'static StaticSchema),
}
#[derive(Clone, Copy, Debug)]
pub struct StaticField {
    pub name: &'static str,
    pub schema: StaticSchema,
}
#[derive(Clone, Copy, Debug)]
pub struct StaticVariant {
    pub name: &'static str,
    pub schema: Option<StaticSchema>,
}
/// Canonical schema bytes, with no wire profile prefix or padding in `bytes()`.
#[derive(Clone, Copy)]
pub struct SchemaBytes {
    data: [u8; 16384],
    len: usize,
}
impl SchemaBytes {
    pub fn bytes(&self) -> &[u8] {
        &self.data[..self.len]
    }
    pub const fn len(&self) -> usize {
        self.len
    }
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub const fn at(&self, index: usize) -> u8 {
        assert!(index < self.len);
        self.data[index]
    }
    const fn byte(&mut self, v: u8) {
        assert!(self.len < self.data.len(), "schema exceeds profile bound");
        self.data[self.len] = v;
        self.len += 1;
    }
    const fn raw(&mut self, bytes: &[u8]) {
        let mut i = 0;
        while i < bytes.len() {
            self.byte(bytes[i]);
            i += 1;
        }
    }
    const fn count(&mut self, n: usize) {
        assert!(n <= crate::MAX_COUNT, "schema count limit");
        self.raw(&(n as u32).to_be_bytes());
    }
    const fn key(&mut self, s: &str) {
        self.count(s.len());
        self.raw(s.as_bytes());
    }
    const fn text(&mut self, s: &str) {
        self.byte(5);
        self.key(s);
    }
    const fn schema(&mut self, s: StaticSchema, depth: usize) {
        assert!(depth <= crate::MAX_DEPTH, "schema depth limit");
        match s {
            StaticSchema::List(inner) | StaticSchema::Option(inner) => {
                self.byte(7);
                self.count(2);
                self.text(if matches!(s, StaticSchema::List(_)) {
                    "List"
                } else {
                    "Option"
                });
                self.schema(*inner, depth + 1);
            }
            StaticSchema::Struct(fields) => {
                self.byte(7);
                self.count(2);
                self.text("Struct");
                self.byte(8);
                self.count(fields.len());
                let mut i = 0;
                while i < fields.len() {
                    if i > 0 {
                        assert!(
                            compare(fields[i - 1].name, fields[i].name) < 0,
                            "schema fields must be sorted and unique"
                        );
                    }
                    self.key(fields[i].name);
                    self.schema(fields[i].schema, depth + 1);
                    i += 1;
                }
            }
            StaticSchema::Variants(names) => {
                assert!(!names.is_empty(), "empty variants");
                self.byte(7);
                self.count(2);
                self.text("Variants");
                self.byte(7);
                self.count(names.len());
                let mut i = 0;
                while i < names.len() {
                    if i > 0 {
                        assert!(
                            compare(names[i - 1], names[i]) < 0,
                            "variants must be sorted and unique"
                        );
                    }
                    self.text(names[i]);
                    i += 1;
                }
            }
            StaticSchema::Tagged(variants) => {
                assert!(!variants.is_empty(), "empty tagged variants");
                self.byte(7);
                self.count(2);
                self.text("Tagged");
                self.byte(8);
                self.count(variants.len());
                let mut i = 0;
                while i < variants.len() {
                    if i > 0 {
                        assert!(
                            compare(variants[i - 1].name, variants[i].name) < 0,
                            "tagged variants must be sorted and unique"
                        );
                    }
                    self.key(variants[i].name);
                    match variants[i].schema {
                        Some(schema) => self.schema(schema, depth + 1),
                        None => self.byte(0),
                    }
                    i += 1;
                }
            }
            StaticSchema::Ref(inner) => {
                self.byte(7);
                self.count(2);
                self.text("Ref");
                self.byte(6);
                self.count(32);
                self.raw(&inner.id_bytes());
            }
            _ => self.text(match s {
                StaticSchema::Bool => "Bool",
                StaticSchema::I64 => "I64",
                StaticSchema::U64 => "U64",
                StaticSchema::String => "String",
                StaticSchema::Bytes => "Bytes",
                _ => panic!("scalar"),
            }),
        }
    }
}
pub const fn compare(a: &str, b: &str) -> i32 {
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
impl StaticSchema {
    pub const fn encode(self) -> SchemaBytes {
        let mut out = SchemaBytes {
            data: [0; 16384],
            len: 0,
        };
        out.schema(self, 0);
        out
    }
    pub fn owned(self) -> Result<ValueSchema, Failure> {
        ValueSchema::decode(self.encode().bytes())
    }
    pub fn id(self) -> Result<SchemaId, Failure> {
        SchemaId::from_bytes(&self.id_bytes())
    }
    pub const fn id_bytes(self) -> [u8; 32] {
        sha256(self.encode())
    }
}
// SHA-256 over the frozen domain followed by canonical schema bytes. Keeping
// this constant permits offline native metadata without invoking plugin code.
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];
const fn sha256(data: SchemaBytes) -> [u8; 32] {
    let domain = b"phenix.schema.v2\0";
    let len = domain.len() + data.len;
    let total = (len + 9).div_ceil(64) * 64;
    let mut h = [
        0x6a09e667u32,
        0xbb67ae85,
        0x3c6ef372,
        0xa54ff53a,
        0x510e527f,
        0x9b05688c,
        0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut offset = 0;
    while offset < total {
        let mut w = [0u32; 64];
        let mut i = 0;
        while i < 64 {
            let pos = offset + i;
            let byte = if pos < domain.len() {
                domain[pos]
            } else if pos < len {
                data.data[pos - domain.len()]
            } else if pos == len {
                128
            } else if pos >= total - 8 {
                ((len as u64 * 8) >> ((total - 1 - pos) * 8)) as u8
            } else {
                0
            };
            w[i / 4] |= (byte as u32) << ((3 - i % 4) * 8);
            i += 1;
        }
        i = 16;
        while i < 64 {
            let a = w[i - 15];
            let b = w[i - 2];
            w[i] = w[i - 16]
                .wrapping_add(a.rotate_right(7) ^ a.rotate_right(18) ^ (a >> 3))
                .wrapping_add(w[i - 7])
                .wrapping_add(b.rotate_right(17) ^ b.rotate_right(19) ^ (b >> 10));
            i += 1;
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        i = 0;
        while i < 64 {
            let t1 = hh
                .wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25))
                .wrapping_add((e & f) ^ (!e & g))
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let t2 = (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22))
                .wrapping_add((a & b) ^ (a & c) ^ (b & c));
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
            i += 1;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
        offset += 64;
    }
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 8 {
        let bytes = h[i].to_be_bytes();
        let mut j = 0;
        while j < 4 {
            out[i * 4 + j] = bytes[j];
            j += 1;
        }
        i += 1;
    }
    out
}

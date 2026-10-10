use crate::{Code,Failure,Value};
use serde::{Deserialize,Serialize};
use std::collections::BTreeMap;
#[derive(Clone,Copy)] pub enum Schema {Bool,I64,U64,String,Bytes,Struct(&'static [Field]),Variants(&'static [&'static str])}
#[derive(Clone,Copy)] pub struct Field {pub name:&'static str,pub schema:Schema}
#[derive(Clone,Copy)] pub struct OperationDef {pub key:&'static str,pub request:Schema,pub response:Schema,pub error:Schema}
#[derive(Clone,Copy)] pub struct Declaration {pub operation:OperationDef,pub provenance:&'static str}
#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
#[serde(tag="kind",deny_unknown_fields)]
pub enum SchemaSpec {Bool,I64,U64,String,Bytes,Struct{fields:BTreeMap<String,SchemaSpec>},Variants{variants:Vec<String>}}
impl SchemaSpec {
    pub fn accepts(&self,value:&Value)->bool{match(self,value){
        (Self::Bool,Value::Bool(_))|(Self::I64,Value::I64(_))|(Self::U64,Value::U64(_))|(Self::String,Value::String(_))|(Self::Bytes,Value::Bytes(_))=>true,
        (Self::Struct{fields},Value::Map(values))=>fields.len()==values.len()&&fields.iter().all(|(k,s)|values.get(k).is_some_and(|v|s.accepts(v))),
        (Self::Variants{variants},Value::String(v))=>variants.contains(v),_=>false,
    }}
}
#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)] #[serde(deny_unknown_fields)]
pub struct Contract {pub key:String,pub request:SchemaSpec,pub response:SchemaSpec,pub error:SchemaSpec}
#[derive(Debug,Clone,Serialize,Deserialize)] #[serde(deny_unknown_fields)]
pub struct Contribution {pub id:String,pub kind:String,pub operation:String,pub owner:String,pub provenance:String,pub role:String}
#[derive(Debug,Clone,Serialize,Deserialize)] #[serde(deny_unknown_fields)]
pub struct Descriptor {
    pub abi_major:u32,pub abi_minor:u32,pub contracts:Vec<Contract>,pub execution:String,
    pub exports:Vec<Contribution>,pub features:u32,pub format:String,pub imports:Vec<Contribution>,
    pub plugin_id:String,pub plugin_version:String,pub schema_version:u32,
}
fn identifier(id:&str)->bool{!id.is_empty()&&id.len()<=256&&id.bytes().all(|c|c.is_ascii_alphanumeric()||b"._-".contains(&c))}
pub fn operation_key(key:&str)->bool{let Some((interface,operation))=key.split_once('/') else{return false;};let Some((name,major))=interface.rsplit_once('@')else{return false;};identifier(name)&&identifier(operation)&&major.parse::<u32>().is_ok_and(|m|m>0&&major==m.to_string())}
impl Descriptor {
    pub fn decode(bytes:&[u8])->Result<Self,Failure>{
        let fail=|m:String|Failure::new(Code::DescriptorInvalid,m);
        let v=Value::decode(bytes).map_err(|e|fail(e.to_string()))?;
        if v.encode().map_err(|e|fail(e.to_string()))?!=bytes{return Err(fail("noncanonical descriptor".into()));}
        let d:Self=serde_json::from_value(v.to_json()).map_err(|e|fail(e.to_string()))?;
        if d.format!="phenix.descriptor.v1"||d.schema_version!=1||d.execution!="sync"||!identifier(&d.plugin_id)||d.plugin_version.is_empty(){return Err(fail("invalid descriptor profile or identity".into()));}
        if d.abi_major!=1||d.abi_minor!=0||d.features!=0{return Err(Failure::new(Code::AbiIncompatible,"unsupported offline ABI requirements"));}
        let mut previous="";
        for c in &d.contracts{if !operation_key(&c.key)||c.key.as_str()<=previous{return Err(fail("invalid or duplicate contract key".into()));}previous=&c.key;}
        for(role,items)in [("Provide",&d.exports),("Require",&d.imports)]{
            let mut previous="";
            for c in items{
                if c.id!=format!("{}/{}/{role}",d.plugin_id,c.operation)||c.id.as_str()<=previous||c.owner!=d.plugin_id||c.role!=role||c.kind!="phenix.operation.v1"||c.provenance.is_empty()||!d.contracts.iter().any(|op|op.key==c.operation){return Err(fail("invalid contribution identity, owner or schema".into()));}previous=&c.id;
            }
        }
        if d.contracts.iter().any(|c|!d.exports.iter().chain(&d.imports).any(|x|x.operation==c.key)){return Err(fail("undeclared contract".into()));}
        Ok(d)
    }
    pub fn contract(&self,key:&str)->Option<&Contract>{self.contracts.iter().find(|x|x.key==key)}
    pub fn permits(&self,key:&str)->bool{self.contract(key).is_some()}
}
// Constant evaluation is the shared lowering mechanism. The macro supplies typed
// declarations; it never reads source files or maintains a metadata registry.
pub const SECTION_BYTES:usize=16*1024;
struct Writer {bytes:[u8;SECTION_BYTES],at:usize}
impl Writer {
    const fn byte(&mut self,b:u8){assert!(self.at<SECTION_BYTES,"descriptor exceeds 16 KiB");self.bytes[self.at]=b;self.at+=1;}
    const fn raw(&mut self,s:&str){let b=s.as_bytes();let mut i=0;while i<b.len(){self.byte(b[i]);i+=1;}}
    const fn n(&mut self,n:usize){let b=(n as u32).to_be_bytes();let mut i=0;while i<4{self.byte(b[i]);i+=1;}}
    const fn key(&mut self,s:&str){self.n(s.len());self.raw(s);}
    const fn text(&mut self,s:&str){self.byte(5);self.key(s);}
    const fn u64(&mut self,n:u64){self.byte(4);let b=n.to_be_bytes();let mut i=0;while i<8{self.byte(b[i]);i+=1;}}
    const fn map(&mut self,n:usize){self.byte(8);self.n(n);}
    const fn list(&mut self,n:usize){self.byte(7);self.n(n);}
    const fn schema(&mut self,s:Schema){match s{
        Schema::Struct(fields)=>{self.map(2);self.key("fields");self.map(fields.len());let mut i=0;while i<fields.len(){if i>0{assert!(compare(fields[i-1].name,fields[i].name)<0,"schema fields must be sorted");}self.key(fields[i].name);self.schema(fields[i].schema);i+=1;}self.key("kind");self.text("Struct");},
        Schema::Variants(variants)=>{self.map(2);self.key("kind");self.text("Variants");self.key("variants");self.list(variants.len());let mut i=0;while i<variants.len(){self.text(variants[i]);i+=1;}},
        other=>{self.map(1);self.key("kind");self.text(match other{Schema::Bool=>"Bool",Schema::I64=>"I64",Schema::U64=>"U64",Schema::String=>"String",Schema::Bytes=>"Bytes",_=>panic!("exhaustive schema")});},
    }}
    const fn contribution(&mut self,owner:&str,d:Declaration,role:&str){
        self.map(6);self.key("id");self.byte(5);self.n(owner.len()+d.operation.key.len()+role.len()+2);self.raw(owner);self.raw("/");self.raw(d.operation.key);self.raw("/");self.raw(role);
        self.key("kind");self.text("phenix.operation.v1");self.key("operation");self.text(d.operation.key);self.key("owner");self.text(owner);self.key("provenance");self.text(d.provenance);self.key("role");self.text(role);
    }
}
const fn compare(a:&str,b:&str)->i32{let(a,b)=(a.as_bytes(),b.as_bytes());let mut i=0;while i<a.len()&&i<b.len(){if a[i]<b[i]{return -1;}if a[i]>b[i]{return 1;}i+=1;}if a.len()<b.len(){-1}else if a.len()>b.len(){1}else{0}}
const fn sort<const N:usize>(mut items:[Declaration;N])->[Declaration;N]{let mut i=1;while i<N{let mut j=i;while j>0&&compare(items[j-1].operation.key,items[j].operation.key)>0{let x=items[j];items[j]=items[j-1];items[j-1]=x;j-=1;}i+=1;}items}
pub const fn descriptor<const I:usize,const E:usize>(id:&str,version:&str,imports:[Declaration;I],exports:[Declaration;E])->[u8;SECTION_BYTES]{
    let imports=sort(imports);let exports=sort(exports);
    let mut w=Writer{bytes:[0;SECTION_BYTES],at:4};w.map(11);
    w.key("abi_major");w.u64(1);w.key("abi_minor");w.u64(0);w.key("contracts");
    let mut count=I+E;let mut i=0;while i<I{let mut j=0;while j<E{if compare(imports[i].operation.key,exports[j].operation.key)==0{count-=1;}j+=1;}i+=1;}
    w.list(count);let(mut i,mut j)=(0,0);
    while i<I||j<E{let op=if j==E||(i<I&&compare(imports[i].operation.key,exports[j].operation.key)<0){let op=imports[i].operation;i+=1;op}else{let op=exports[j].operation;j+=1;if i<I&&compare(imports[i].operation.key,op.key)==0{i+=1;}op};
        w.map(4);w.key("error");w.schema(op.error);w.key("key");w.text(op.key);w.key("request");w.schema(op.request);w.key("response");w.schema(op.response);
    }
    w.key("execution");w.text("sync");w.key("exports");w.list(E);let mut i=0;while i<E{w.contribution(id,exports[i],"Provide");i+=1;}
    w.key("features");w.u64(0);w.key("format");w.text("phenix.descriptor.v1");w.key("imports");w.list(I);let mut i=0;while i<I{w.contribution(id,imports[i],"Require");i+=1;}
    w.key("plugin_id");w.text(id);w.key("plugin_version");w.text(version);w.key("schema_version");w.u64(1);
    let n=((w.at-4)as u32).to_be_bytes();let mut i=0;while i<4{w.bytes[i]=n[i];i+=1;}w.bytes
}

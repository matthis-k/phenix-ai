use phenix_contract::{Code,Descriptor,Failure,SECTION_BYTES};
use object::{Object,ObjectSection};
use sha2::{Digest,Sha256};
pub fn digest(bytes:&[u8])->String{format!("{:x}",Sha256::digest(bytes))}
pub fn extract(bytes:&[u8])->Result<Vec<u8>,Failure>{
    let invalid=|m:String|Failure::new(Code::DescriptorInvalid,m);
    let object=object::File::parse(bytes).map_err(|e|invalid(e.to_string()))?;
    if object.format()!=object::BinaryFormat::Elf||object.kind()!=object::ObjectKind::Dynamic||object.architecture()!=object::Architecture::X86_64||!object.is_little_endian(){return Err(Failure::new(Code::ArtifactInvalid,"expected x86_64 Linux ELF shared library"));}
    let mut sections=object.sections().filter(|s|s.name().ok()==Some(".phenix"));
    let section=sections.next().ok_or_else(||invalid("missing .phenix descriptor section".into()))?;
    if sections.next().is_some(){return Err(invalid("duplicate descriptor section".into()));}
    let data=section.data().map_err(|e|invalid(e.to_string()))?;
    if data.len()!=SECTION_BYTES{return Err(invalid("invalid descriptor section size".into()));}
    let size=u32::from_be_bytes(data[..4].try_into().map_err(|_|invalid("descriptor length".into()))?)as usize;
    let end=4usize.checked_add(size).ok_or_else(||invalid("descriptor overflow".into()))?;
    let payload=data.get(4..end).ok_or_else(||invalid("descriptor outside section".into()))?;
    if data[end..].iter().any(|&b|b!=0){return Err(invalid("nonzero descriptor padding".into()));}
    Descriptor::decode(payload)?;Ok(payload.to_vec())
}

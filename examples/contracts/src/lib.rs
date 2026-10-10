use phenix_sdk::{operation,Portable};
#[derive(Debug,PartialEq,Eq,Portable)] pub struct Text {pub text:String}
#[derive(Debug,PartialEq,Eq,Portable)] pub struct Count {pub count:u64}
#[derive(Debug,PartialEq,Eq,Portable)] pub enum TransformError {Empty}
#[operation(interface="example.transform@1",operation="apply")]
pub type Transform=(Text,Text,TransformError);
#[operation(interface="example.consumer@1",operation="run")]
pub type Run=(Text,Text,TransformError);
#[operation(interface="example.transform@1",operation="count")]
pub type ReadCount=(Count,Count,TransformError);

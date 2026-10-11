use phenix_sdk::{operation, Portable};
#[derive(Debug, PartialEq, Eq, Portable)]
pub struct Text {
    pub text: String,
}
#[derive(Debug, PartialEq, Eq, Portable)]
pub struct Count {
    pub count: u64,
}
#[derive(Debug, PartialEq, Eq, Portable)]
pub enum TransformError {
    Empty,
}
#[operation(interface = "example.transform@1", operation = "apply")]
pub type Transform = (Text, Text, TransformError);
#[operation(interface = "example.consumer@1", operation = "run")]
pub type Run = (Text, Text, TransformError);
#[operation(interface = "example.transform@1", operation = "count")]
pub type ReadCount = (Count, Count, TransformError);

#[operation(interface = "example.choice@1", operation = "run")]
pub type Choice = (Text, Text, TransformError);

#[operation(interface = "example.other@1", operation = "run")]
pub type Other = (Text, Text, TransformError);

#[operation(interface = "example.restricted@1", operation = "run")]
pub type ScopedRun = (Text, Count, TransformError);

#[operation(interface = "example.relay@1", operation = "run")]
pub type Relay = (Text, Count, TransformError);

#[operation(interface = "example.resource@1", operation = "bump")]
pub type Bump = (Count, Count, TransformError);

#[operation(interface = "example.resource@1", operation = "count")]
pub type ResourceCount = (Count, Count, TransformError);

#![forbid(unsafe_code)]

#[tokio::main]
async fn main() -> Result<(), phenix_harness::application::ConfiguredApplicationError> {
    phenix_harness::application::serve_default_application().await
}

//! Independent official SDK negotiation check for the gated v1/v2 router.
//! This checks only initialization, not the unfinished v2 session lifecycle.
use agent_client_protocol::schema::{v1, v2, ProtocolVersion};
use agent_client_protocol::{AcpAgent, Agent, Client, ConnectionTo};
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let command: Vec<String> = std::env::args().skip(1).collect();
    if command.is_empty() {
        return Err("usage: v2-init-preview PHENIX_RUNTIME acp COMPOSITION".into());
    }

    Client
        .v2()
        .name("phenix-v2-preview-sdk-client")
        .connect_with(
            AcpAgent::from_args(command.clone())?,
            async |connection| {
                let response = connection
                    .send_request(v2::InitializeRequest::new(
                        ProtocolVersion::V2,
                        v2::Implementation::new("phenix-v2-preview-test", "0.1.0"),
                    ))
                    .block_task()
                    .await?;
                assert_eq!(response.protocol_version, ProtocolVersion::V2);
                assert!(
                    response.capabilities.session.is_none(),
                    "preview must not advertise an unfinished v2 session lifecycle"
                );
                Ok(())
            },
        )
        .await?;

    // The same opt-in binary must still route a stable-v1 client to the
    // single tested Phenix v1 host, with no v2 schema in its responses.
    Client
        .builder()
        .name("phenix-v1-preview-regression")
        .connect_with(
            AcpAgent::from_args(command)?,
            |connection: ConnectionTo<Agent>| async move {
                let response = connection
                    .send_request(v1::InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;
                assert_eq!(response.protocol_version, ProtocolVersion::V1);
                Ok(())
            },
        )
        .await?;

    println!("official-acp-sdk-preview: v1/v2 initialization and capability boundary");
    Ok(())
}

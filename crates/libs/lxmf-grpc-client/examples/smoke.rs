use lxmf_grpc_client::lxmf::common::v1::PageRequest;
use lxmf_grpc_client::lxmf::identity::v1::ListIdentitiesRequest;
use lxmf_grpc_client::lxmf::runtime::v1::GetSnapshotRequest;
use lxmf_grpc_client::lxmf::topics::v1::ListTopicsRequest;
use lxmf_grpc_client::LxmfGrpcClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = std::env::var("LXMF_GRPC_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:50051".to_string());
    let mut builder = LxmfGrpcClient::builder(endpoint);
    if let Ok(token) = std::env::var("LXMF_GRPC_BEARER_TOKEN") {
        if !token.trim().is_empty() {
            builder = builder.bearer_token(token);
        }
    }
    let client = builder.connect().await?;

    let snapshot = client
        .runtime()
        .get_snapshot(GetSnapshotRequest { include_counts: true })
        .await?
        .into_inner();
    println!(
        "runtime: id={} state={} contract=v{}",
        snapshot.runtime_id, snapshot.state, snapshot.active_contract_version
    );

    let topics = client
        .topics()
        .list_topics(ListTopicsRequest {
            page: Some(PageRequest { page_token: String::new(), page_size: 10 }),
        })
        .await?
        .into_inner();
    println!("topics: {} returned", topics.topics.len());

    let identities =
        client.identity().list_identities(ListIdentitiesRequest {}).await?.into_inner();
    println!("identities: {} returned", identities.identities.len());

    Ok(())
}

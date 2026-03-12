use lxmf_grpc_client::lxmf::runtime::v1::GetSnapshotRequest;
use lxmf_grpc_client::LxmfGrpcClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = std::env::var("LXMF_GRPC_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:50051".to_string());

    let client = match std::env::var("LXMF_GRPC_BEARER_TOKEN") {
        Ok(token) if !token.trim().is_empty() => {
            LxmfGrpcClient::builder(endpoint).bearer_token(token).connect().await?
        }
        _ => LxmfGrpcClient::connect(endpoint).await?,
    };

    let snapshot = client
        .runtime()
        .get_snapshot(GetSnapshotRequest { include_counts: true })
        .await?
        .into_inner();

    println!("runtime_id={}", snapshot.runtime_id);
    println!("state={}", snapshot.state);
    println!("contract_version={}", snapshot.active_contract_version);
    println!(
        "capabilities={}",
        snapshot.effective_capabilities.join(",")
    );

    Ok(())
}

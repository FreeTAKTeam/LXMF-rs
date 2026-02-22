use lxmf_sdk::{
    Client, LxmfSdk, LxmfSdkManualTick, RpcBackendClient, SdkConfig, StartRequest, TickBudget,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = std::env::var("LXMF_RPC").unwrap_or_else(|_| "127.0.0.1:4242".to_owned());
    let client = Client::new(RpcBackendClient::new(endpoint.clone()));

    let start_request =
        StartRequest::new(SdkConfig::embedded_alloc_default().with_rpc_listen_addr(endpoint));

    let handle = client.start(start_request)?;
    println!("embedded profile started: runtime_id={}", handle.runtime_id);

    let budget = TickBudget::new(64).with_max_duration_ms(25);
    let tick_result = client.tick(budget)?;
    println!(
        "tick processed_items={} yielded={} next_delay_ms={:?}",
        tick_result.processed_items, tick_result.yielded, tick_result.next_recommended_delay_ms
    );

    Ok(())
}

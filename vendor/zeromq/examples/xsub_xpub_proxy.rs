mod async_helpers;

use zeromq::{proxy, Socket, XPubSocket, XSubSocket};

#[async_helpers::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    pretty_env_logger::try_init().ok();

    let mut frontend = XSubSocket::new();
    let mut backend = XPubSocket::new();

    let frontend_endpoint = frontend.bind("tcp://127.0.0.1:5557").await?;
    let backend_endpoint = backend.bind("tcp://127.0.0.1:5558").await?;

    println!("XSUB/XPUB proxy running");
    println!(
        "Upstream publishers or brokers connect to {}",
        frontend_endpoint
    );
    println!(
        "Downstream subscribers or brokers connect to {}",
        backend_endpoint
    );

    proxy(frontend, backend, None).await?;
    Ok(())
}

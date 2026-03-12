use clap::{Args as ClapArgs, Parser, Subcommand};
use lxmf_grpc_client::lxmf::admin::v1::ListInterfacesRequest;
use lxmf_grpc_client::lxmf::common::v1::PageRequest;
use lxmf_grpc_client::lxmf::events::v1::PollEventsRequest;
use lxmf_grpc_client::lxmf::markers::v1::ListMarkersRequest;
use lxmf_grpc_client::lxmf::runtime::v1::GetSnapshotRequest;
use lxmf_grpc_client::lxmf::topics::v1::ListTopicsRequest;
use lxmf_grpc_client::LxmfGrpcClient;

#[derive(Parser, Debug)]
#[command(name = "rngrpc")]
#[command(about = "Small gRPC operator wrapper for reticulumd")]
struct Cli {
    #[arg(long)]
    endpoint: Option<String>,
    #[arg(long)]
    bearer_token: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Snapshot(SnapshotArgs),
    #[command(subcommand)]
    Topics(TopicsCommand),
    #[command(subcommand)]
    Interfaces(InterfacesCommand),
    #[command(subcommand)]
    Events(EventsCommand),
    #[command(subcommand)]
    Markers(MarkersCommand),
}

#[derive(ClapArgs, Debug)]
struct SnapshotArgs {
    #[arg(long, default_value_t = true)]
    include_counts: bool,
}

#[derive(Subcommand, Debug)]
enum TopicsCommand {
    List(ListPageArgs),
}

#[derive(Subcommand, Debug)]
enum InterfacesCommand {
    List,
}

#[derive(Subcommand, Debug)]
enum EventsCommand {
    Poll(PollArgs),
}

#[derive(Subcommand, Debug)]
enum MarkersCommand {
    List(ListMarkersArgs),
}

#[derive(ClapArgs, Debug)]
struct ListPageArgs {
    #[arg(long, default_value_t = 25)]
    limit: u32,
    #[arg(long)]
    page_token: Option<String>,
}

#[derive(ClapArgs, Debug)]
struct PollArgs {
    #[arg(long, default_value_t = 16)]
    max: u32,
    #[arg(long)]
    cursor: Option<String>,
}

#[derive(ClapArgs, Debug)]
struct ListMarkersArgs {
    #[arg(long, default_value_t = 25)]
    limit: u32,
    #[arg(long)]
    page_token: Option<String>,
    #[arg(long)]
    topic_id: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let endpoint = cli
        .endpoint
        .or_else(|| std::env::var("LXMF_GRPC_ENDPOINT").ok())
        .unwrap_or_else(|| "http://127.0.0.1:50051".to_string());
    let mut builder = LxmfGrpcClient::builder(endpoint);
    let bearer_token = cli.bearer_token.or_else(|| std::env::var("LXMF_GRPC_BEARER_TOKEN").ok());
    if let Some(token) = bearer_token.filter(|value| !value.trim().is_empty()) {
        builder = builder.bearer_token(token);
    }
    let client = builder.connect().await?;

    match cli.command {
        Command::Snapshot(args) => {
            let response = client
                .runtime()
                .get_snapshot(GetSnapshotRequest { include_counts: args.include_counts })
                .await?
                .into_inner();
            println!("runtime_id: {}", response.runtime_id);
            println!("state: {}", response.state);
            println!("contract_version: {}", response.active_contract_version);
            println!("capabilities: {}", response.effective_capabilities.join(", "));
            println!("config_revision: {}", response.config_revision);
        }
        Command::Topics(TopicsCommand::List(args)) => {
            let response = client
                .topics()
                .list_topics(ListTopicsRequest {
                    page: Some(PageRequest {
                        page_token: args.page_token.unwrap_or_default(),
                        page_size: args.limit,
                    }),
                })
                .await?
                .into_inner();
            for topic in &response.topics {
                println!("{}  {}", topic.topic_id, topic.topic_path);
            }
            if !response
                .page_info
                .as_ref()
                .map(|info| info.next_page_token.is_empty())
                .unwrap_or(true)
            {
                println!(
                    "\nnext_page_token: {}",
                    response.page_info.expect("checked page info").next_page_token
                );
            }
        }
        Command::Interfaces(InterfacesCommand::List) => {
            let response =
                client.admin().list_interfaces(ListInterfacesRequest {}).await?.into_inner();
            for interface in response.interfaces {
                let name = interface.name.unwrap_or_else(|| "<unnamed>".to_string());
                println!("{}  enabled={}  {}", name, interface.enabled, interface.r#type);
            }
        }
        Command::Events(EventsCommand::Poll(args)) => {
            let response = client
                .events()
                .poll_events(PollEventsRequest { cursor: args.cursor, max: args.max })
                .await?
                .into_inner();
            for event in &response.events {
                println!(
                    "{}  seq={}  severity={}  source={}",
                    event.event_type, event.seq_no, event.severity, event.source_component
                );
            }
            if !response.next_cursor.is_empty() {
                println!("\nnext_cursor: {}", response.next_cursor);
            }
            if response.dropped_count > 0 {
                println!("dropped_count: {}", response.dropped_count);
            }
        }
        Command::Markers(MarkersCommand::List(args)) => {
            let response = client
                .markers()
                .list_markers(ListMarkersRequest {
                    topic_id: args.topic_id,
                    page: Some(PageRequest {
                        page_token: args.page_token.unwrap_or_default(),
                        page_size: args.limit,
                    }),
                })
                .await?
                .into_inner();
            for marker in &response.markers {
                println!(
                    "{}  {}  ({:.6}, {:.6})",
                    marker.marker_id,
                    marker.label,
                    marker.position.as_ref().map(|p| p.lat).unwrap_or(0.0),
                    marker.position.as_ref().map(|p| p.lon).unwrap_or(0.0)
                );
            }
            if !response
                .page_info
                .as_ref()
                .map(|info| info.next_page_token.is_empty())
                .unwrap_or(true)
            {
                println!(
                    "\nnext_page_token: {}",
                    response.page_info.expect("checked page info").next_page_token
                );
            }
        }
    }

    Ok(())
}

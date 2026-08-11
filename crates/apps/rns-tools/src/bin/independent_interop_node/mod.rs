mod control;
mod events;
mod model;
mod node;
mod performance;

pub use model::Cli;

pub async fn run(cli: Cli) -> Result<(), String> {
    node::run(cli).await
}

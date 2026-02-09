use clap::Parser;
use lxmf::cli::app::{run_cli, Cli};

fn main() {
    let cli = Cli::parse();
    if let Err(err) = run_cli(cli) {
        eprintln!("lxmf: {err}");
        std::process::exit(1);
    }
}

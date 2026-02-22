use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "rnpath")]
struct Args {
    #[arg(long)]
    config: Option<String>,
}

fn main() {
    let _ = Args::parse();
}

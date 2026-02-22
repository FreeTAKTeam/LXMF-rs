use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "rncp")]
struct Args {
    #[arg(long)]
    config: Option<String>,
}

fn main() {
    let _ = Args::parse();
}

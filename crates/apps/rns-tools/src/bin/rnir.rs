use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "rnir")]
struct Args {
    #[arg(long)]
    config: Option<String>,
}

fn main() {
    let _ = Args::parse();
}

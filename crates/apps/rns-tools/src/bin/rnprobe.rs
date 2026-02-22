use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "rnprobe")]
struct Args {
    #[arg(long)]
    config: Option<String>,
}

fn main() {
    let _ = Args::parse();
}

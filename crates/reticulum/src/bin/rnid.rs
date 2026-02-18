use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "rnid")]
struct Args {
    #[arg(long)]
    config: Option<String>,
}

fn main() {
    let _ = Args::parse();
}

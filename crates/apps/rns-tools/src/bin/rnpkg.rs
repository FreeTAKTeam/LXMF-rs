use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "rnpkg")]
struct Args {
    #[arg(long)]
    config: Option<String>,
}

fn main() {
    let _ = Args::parse();
}

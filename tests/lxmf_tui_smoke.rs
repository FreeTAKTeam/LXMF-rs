use clap::Parser;
use lxmf::cli::app::{Cli, Command};

#[test]
fn parses_tui_command() {
    let cli = Cli::try_parse_from(["lxmf", "tui", "--refresh-ms", "750"]).unwrap();
    match cli.command {
        Command::Tui(cmd) => assert_eq!(cmd.refresh_ms, 750),
        other => panic!("unexpected command: {other:?}"),
    }
}

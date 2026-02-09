use clap::Parser;
use lxmf::cli::app::{
    Cli, Command, MessageAction, MessageCommand, ProfileAction, ProfileCommand,
};

#[test]
fn parses_profile_init_command() {
    let cli = Cli::try_parse_from([
        "lxmf",
        "profile",
        "init",
        "demo",
        "--managed",
        "--rpc",
        "127.0.0.1:5000",
    ])
    .unwrap();

    match cli.command {
        Command::Profile(ProfileCommand {
            action: ProfileAction::Init { name, managed, rpc },
        }) => {
            assert_eq!(name, "demo");
            assert!(managed);
            assert_eq!(rpc.as_deref(), Some("127.0.0.1:5000"));
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parses_message_send_command() {
    let cli = Cli::try_parse_from([
        "lxmf",
        "message",
        "send",
        "--source",
        "0011",
        "--destination",
        "ffee",
        "--content",
        "hello",
        "--title",
        "subject",
        "--method",
        "direct",
        "--include-ticket",
    ])
    .unwrap();

    match cli.command {
        Command::Message(MessageCommand {
            action: MessageAction::Send(args),
        }) => {
            assert_eq!(args.source, "0011");
            assert_eq!(args.destination, "ffee");
            assert_eq!(args.content, "hello");
            assert_eq!(args.title, "subject");
            assert!(args.include_ticket);
            assert!(args.method.is_some());
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

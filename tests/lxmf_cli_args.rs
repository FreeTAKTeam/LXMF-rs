use clap::Parser;
use lxmf::cli::app::{
    Cli, Command, ContactAction, ContactCommand, MessageAction, MessageCommand, PeerAction,
    PeerCommand, ProfileAction, ProfileCommand,
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

#[test]
fn parses_profile_set_command() {
    let cli = Cli::try_parse_from(["lxmf", "profile", "set", "--display-name", "Tommy Operator"])
        .unwrap();

    match cli.command {
        Command::Profile(ProfileCommand {
            action:
                ProfileAction::Set {
                    display_name,
                    clear_display_name,
                    name,
                },
        }) => {
            assert_eq!(display_name.as_deref(), Some("Tommy Operator"));
            assert!(!clear_display_name);
            assert!(name.is_none());
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parses_peer_query_and_exact_flags() {
    let list_cli =
        Cli::try_parse_from(["lxmf", "peer", "list", "--query", "alice", "--limit", "7"]).unwrap();
    match list_cli.command {
        Command::Peer(PeerCommand {
            action: PeerAction::List { query, limit },
        }) => {
            assert_eq!(query.as_deref(), Some("alice"));
            assert_eq!(limit, Some(7));
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let show_cli = Cli::try_parse_from(["lxmf", "peer", "show", "abc123", "--exact"]).unwrap();
    match show_cli.command {
        Command::Peer(PeerCommand {
            action: PeerAction::Show { selector, exact },
        }) => {
            assert_eq!(selector, "abc123");
            assert!(exact);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parses_contact_commands() {
    let add_cli = Cli::try_parse_from([
        "lxmf",
        "contact",
        "add",
        "alice",
        "0123456789abcdef0123456789abcdef",
        "--notes",
        "friend",
    ])
    .unwrap();
    match add_cli.command {
        Command::Contact(ContactCommand {
            action: ContactAction::Add(args),
        }) => {
            assert_eq!(args.alias, "alice");
            assert_eq!(args.hash, "0123456789abcdef0123456789abcdef");
            assert_eq!(args.notes.as_deref(), Some("friend"));
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let list_cli =
        Cli::try_parse_from(["lxmf", "contact", "list", "--query", "ali", "--limit", "5"]).unwrap();
    match list_cli.command {
        Command::Contact(ContactCommand {
            action: ContactAction::List { query, limit },
        }) => {
            assert_eq!(query.as_deref(), Some("ali"));
            assert_eq!(limit, Some(5));
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

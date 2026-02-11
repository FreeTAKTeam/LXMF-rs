use lxmf::cli::app::{
    Cli, Command, ContactAction, ContactCommand, ContactUpsertArgs, RuntimeContext,
};
use lxmf::cli::commands_contact;
use lxmf::cli::contacts::load_contacts;
use lxmf::cli::output::Output;
use lxmf::cli::profile::{init_profile, load_profile_settings, profile_paths};
use lxmf::cli::rpc_client::RpcClient;

#[test]
fn contact_add_show_remove_roundtrip() {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());
    init_profile("contact-test", false, None).unwrap();

    let settings = load_profile_settings("contact-test").unwrap();
    let ctx = RuntimeContext {
        cli: Cli {
            profile: "contact-test".into(),
            rpc: None,
            json: true,
            quiet: true,
            command: Command::Contact(ContactCommand {
                action: ContactAction::List {
                    query: None,
                    limit: None,
                },
            }),
        },
        profile_name: "contact-test".into(),
        profile_settings: settings.clone(),
        profile_paths: profile_paths("contact-test").unwrap(),
        rpc: RpcClient::new(&settings.rpc),
        output: Output::new(true, true),
    };

    commands_contact::run(
        &ctx,
        &ContactCommand {
            action: ContactAction::Add(ContactUpsertArgs {
                alias: "alice".into(),
                hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                notes: Some("friend".into()),
            }),
        },
    )
    .unwrap();

    let loaded = load_contacts("contact-test").unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].alias, "alice");

    commands_contact::run(
        &ctx,
        &ContactCommand {
            action: ContactAction::Show {
                selector: "alice".into(),
                exact: false,
            },
        },
    )
    .unwrap();

    commands_contact::run(
        &ctx,
        &ContactCommand {
            action: ContactAction::Remove {
                selector: "alice".into(),
                exact: true,
            },
        },
    )
    .unwrap();

    assert!(load_contacts("contact-test").unwrap().is_empty());
    std::env::remove_var("LXMF_CONFIG_ROOT");
}

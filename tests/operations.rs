use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use anyhow::Result;
use clap::Parser;
use serde_json::{Value, json};
use taxudromeio::{
    cli::{self, Cli, Command, MailCommand},
    config::{Account, AuthType, Config},
    mail::{MailBackend, MailRequest},
    policy::{Policy, Profile},
    server::MailServer,
    service::{MailService, configurations},
};

struct FixtureBackend(AtomicUsize);

impl MailBackend for FixtureBackend {
    fn execute(&self, account: &Account, request: MailRequest) -> Result<Value> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let operation = request.operation().tool_name();
        let arguments = match request {
            MailRequest::Folders(args) | MailRequest::Status(args) => serde_json::to_value(args)?,
            MailRequest::List(args) => serde_json::to_value(args)?,
            MailRequest::Search(args) => serde_json::to_value(args)?,
            MailRequest::Read(args) => serde_json::to_value(args)?,
            MailRequest::Move(args) | MailRequest::Copy(args) => serde_json::to_value(args)?,
            MailRequest::Flags(args) => serde_json::to_value(args)?,
            MailRequest::Draft(args) => serde_json::to_value(args)?,
        };
        Ok(json!({"configuration": account.id, "operation": operation, "arguments": arguments}))
    }
}

fn account(id: &str, profile: Profile) -> Account {
    Account {
        id: id.into(),
        host: "imap.example.test".into(),
        port: 993,
        transport: Default::default(),
        root: "Projects".into(),
        drafts_folder: Some("Drafts".into()),
        auth_type: AuthType::Password,
        policy: Policy {
            profile,
            allowed_operations: None,
        },
    }
}

fn parse(args: &[&str]) -> MailCommand {
    let parsed = Cli::try_parse_from(
        ["taxudromeio", "mail"]
            .into_iter()
            .chain(args.iter().copied()),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    match parsed.command.unwrap() {
        Command::Mail(command) => command,
        _ => panic!("Expected a mail command"),
    }
}

/// anchorbook: A-interface-shared#adapter-parity
#[tokio::test]
async fn all_cli_operations_match_mcp_arguments_and_results() {
    let backend = Arc::new(FixtureBackend(AtomicUsize::new(0)));
    let service = MailService::new(
        Config {
            accounts: vec![account("work", Profile::Triage)],
            ..Config::default()
        },
        backend.clone(),
    )
    .unwrap();
    let server = MailServer::from_service(service.clone());
    for (arguments, tool, expected) in [
        (
            "folders --configuration work",
            "mail_list_folders",
            json!({"configuration": "work", "folder": "."}),
        ),
        (
            "status --configuration work --folder Café",
            "mail_folder_status",
            json!({"configuration": "work", "folder": "Café"}),
        ),
        (
            "list --configuration work --folder Archive --limit 25 --before-uid 100",
            "mail_list_messages",
            json!({"configuration": "work", "folder": "Archive", "limit": 25, "before_uid": 100}),
        ),
        (
            "search --configuration work --limit 10 --before-uid 100 --from sender --to recipient --subject invoice --text needle --unread --flagged=false --since 2026-01-01 --before 2026-02-01",
            "mail_search",
            json!({"configuration": "work", "limit": 10, "before_uid": 100, "from": "sender", "to": "recipient",
                "subject": "invoice", "text": "needle", "unread": true, "flagged": false,
                "since": "2026-01-01", "before": "2026-02-01"}),
        ),
        (
            "read --configuration work --folder Archive --uid 12 --uid-validity 7",
            "mail_read_message",
            json!({"configuration": "work", "folder": "Archive", "uid": 12, "uid_validity": 7}),
        ),
        (
            "move --configuration work --destination Done --uids 12,13 --uid-validity 7",
            "mail_move_messages",
            json!({"configuration": "work", "destination": "Done", "uids": [12,13], "uid_validity": 7}),
        ),
        (
            "copy --configuration work --folder Archive --destination Done --uids 12 13 --uid-validity 7",
            "mail_copy_messages",
            json!({"configuration": "work", "folder": "Archive", "destination": "Done", "uids": [12,13], "uid_validity": 7}),
        ),
        (
            "flags --configuration work --uids 12 --uids 13 --uid-validity 7 --action remove --flags seen,flagged --flags answered draft",
            "mail_set_flags",
            json!({"configuration": "work", "uids": [12,13], "uid_validity": 7, "action": "remove", "flags": ["seen","flagged","answered","draft"]}),
        ),
        (
            "draft --configuration work --folder Drafts --from me@example.test --to you@example.test --to other@example.test --cc cc@example.test --bcc bcc@example.test --subject Draft --body Hello --in-reply-to <id@example.test>",
            "mail_save_draft",
            json!({"configuration": "work", "folder": "Drafts", "from": "me@example.test", "to": ["you@example.test","other@example.test"],
                "cc": ["cc@example.test"], "bcc": ["bcc@example.test"], "subject": "Draft", "body": "Hello", "in_reply_to": "<id@example.test>"}),
        ),
    ] {
        let cli = cli::run_mail(
            parse(&arguments.split_whitespace().collect::<Vec<_>>()),
            &service,
        )
        .await
        .unwrap();
        let mcp = server.invoke(tool, expected.clone()).await.unwrap();
        assert_eq!(cli, mcp, "{arguments}");
        assert_eq!(cli["operation"], tool);
        // Assertions against independently specified arguments catch discarded CLI values.
        for (key, value) in expected.as_object().unwrap() {
            assert_eq!(&cli["arguments"][key], value, "{tool}: {key}");
        }
    }
    assert_eq!(backend.0.load(Ordering::SeqCst), 18);

    let defaults = cli::run_mail(parse(&["list", "--configuration", "work"]), &service)
        .await
        .unwrap();
    assert_eq!(defaults["arguments"]["folder"], ".");
    assert_eq!(defaults["arguments"]["limit"], 50);
    assert_eq!(defaults["arguments"]["before_uid"], Value::Null);

    let body = "Unicode café\n$(this is message data); 'quoted'";
    let draft = cli::run_mail(
        parse(&[
            "draft",
            "--configuration",
            "work",
            "--from",
            "me@example.test",
            "--subject",
            "A subject with spaces",
            "--body",
            body,
        ]),
        &service,
    )
    .await
    .unwrap();
    assert_eq!(draft["arguments"]["body"], body);
    assert_eq!(draft["arguments"]["subject"], "A subject with spaces");
}

/// anchorbook: A-interface-shared#policy-boundary
/// anchorbook: A-policy-triage#direct-call-enforcement
#[tokio::test]
async fn both_adapters_share_discovery_and_refuse_disabled_operations() {
    let backend = Arc::new(FixtureBackend(AtomicUsize::new(0)));
    let mut narrowed = account("narrowed", Profile::Triage);
    narrowed.policy.allowed_operations = Some(Default::default());
    let config = Config {
        accounts: vec![
            account("triage", Profile::Triage),
            account("archive", Profile::ReadOnly),
            narrowed,
        ],
        ..Config::default()
    };
    let expected_discovery = configurations(&config);
    let service = MailService::new(config, backend.clone()).unwrap();
    let server = MailServer::from_service(service.clone());
    assert_eq!(
        server
            .invoke("mail_list_configurations", json!({}))
            .await
            .unwrap(),
        expected_discovery
    );
    for id in ["archive", "narrowed", "missing"] {
        let cli_error = cli::run_mail(
            parse(&[
                "move",
                "--configuration",
                id,
                "--uids",
                "12",
                "--uid-validity",
                "7",
                "--destination",
                "Done",
            ]),
            &service,
        )
        .await
        .err()
        .unwrap();
        let mcp_error = server
            .invoke(
                "mail_move_messages",
                json!({"configuration": id, "uids": [12], "uid_validity": 7, "destination": "Done"}),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(cli_error.to_string(), mcp_error.to_string());
    }
    assert_eq!(backend.0.load(Ordering::SeqCst), 0);
}

/// anchorbook: A-interface-shared#clap-boundary
#[tokio::test]
async fn clap_and_shared_validation_refuse_invalid_or_unsupported_mail_arguments() {
    for invalid in [
        "list",
        "list --configuration work --limit 0",
        "list --configuration work --limit 101",
        "list --configuration work --before-uid 0",
        "read --configuration work --uid 1",
        "read --configuration work --uid 0 --uid-validity 7",
        "read --configuration work --uid 1 --uid-validity 0",
        "move --configuration work --uids 1 --uid-validity 7",
        "move --configuration work --uids 1,0 --uid-validity 7 --destination Done",
        "copy --configuration work --uid-validity 7 --destination Done",
        "flags --configuration work --uids 1 --uid-validity 7 --action add --flags deleted",
        "flags --configuration work --uids 1 --uid-validity 7 --action replace --flags seen",
        "flags --configuration work --uids 1 --uid-validity 7 --action add",
        "search --configuration work --unread=perhaps",
        "folders --configuration work --password fake",
        "folders --configuration work --username fake",
        "send --configuration work",
        "raw --configuration work",
    ] {
        assert!(
            Cli::try_parse_from(
                ["taxudromeio", "mail"]
                    .into_iter()
                    .chain(invalid.split_whitespace())
            )
            .is_err(),
            "Unexpectedly accepted: {invalid}"
        );
    }
    let too_many = (1..=101)
        .map(|uid| uid.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let backend = Arc::new(FixtureBackend(AtomicUsize::new(0)));
    let service = MailService::new(
        Config {
            accounts: vec![account("work", Profile::Triage)],
            ..Config::default()
        },
        backend.clone(),
    )
    .unwrap();
    let error = cli::run_mail(
        parse(&[
            "move",
            "--configuration",
            "work",
            "--uids",
            &too_many,
            "--uid-validity",
            "7",
            "--destination",
            "Done",
        ]),
        &service,
    )
    .await
    .err()
    .unwrap();
    assert!(error.to_string().contains("Supply 1–100 UIDs"));
    let error = cli::run_mail(
        parse(&[
            "flags",
            "--configuration",
            "work",
            "--uids",
            "1",
            "--uid-validity",
            "7",
            "--action",
            "add",
            "--flags",
            "seen",
            "--flags",
            "flagged",
            "--flags",
            "answered",
            "--flags",
            "draft",
            "--flags",
            "seen",
        ]),
        &service,
    )
    .await
    .err()
    .unwrap();
    assert!(error.to_string().contains("Supply 1–4 supported flags"));
    let server = MailServer::from_service(service);
    assert!(
        server
            .invoke(
                "mail_move_messages",
                json!({
                    "configuration": "work", "uids": (1..=101).collect::<Vec<_>>(),
                    "uid_validity": 7, "destination": "Done",
                })
            )
            .await
            .is_err()
    );
    assert_eq!(backend.0.load(Ordering::SeqCst), 0);
    assert!(Cli::try_parse_from(["taxudromeio", "auth", "--password", "fake"]).is_err());
    assert!(Cli::try_parse_from(["taxudromeio", "auth", "--username", "fake"]).is_err());
}

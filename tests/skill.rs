use std::process::Command;

use clap::Parser;
use serde_json::Value;
use taxudromeio::{
    cli::{Cli, Command as ParsedCommand},
    policy::Operation,
};

fn printed_guide() -> String {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_taxudromeio"))
        .current_dir(dir.path())
        .arg("--config")
        .arg(dir.path().join("no-settings.toml"))
        .arg("skill")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    String::from_utf8(output.stdout).unwrap()
}

/// anchorbook: A-cli-skill#standalone-guide
#[test]
fn standalone_skill_prints_detailed_markdown_without_settings() {
    let guide = printed_guide();
    assert!(guide.starts_with("# Using Taxudromeio\n"));
    let prose = guide.split_whitespace().collect::<Vec<_>>().join(" ");
    for guidance in [
        "Login normally happens in the user's CLI",
        "download a mail configuration profile",
        "launch the auth command",
        "hand the terminal to the user",
        "Keep passwords and tokens out of shell arguments",
        "macOS Keychain",
        "UIDVALIDITY",
        "next_before_uid",
        "Use multiple configurations when boundaries differ",
        "same request structs, shared operation service and IMAP backend",
        "fresh IMAP connection",
        "There is no connection pool",
        "MCP is not inherently cheaper in tokens",
        "No operation sends mail",
    ] {
        assert!(
            prose.to_lowercase().contains(&guidance.to_lowercase()),
            "Missing guidance: {guidance}"
        );
    }
    let help = Command::new(env!("CARGO_BIN_EXE_taxudromeio"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(guide.len() > help.stdout.len());
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("taxudromeio skill")
    );
    let output = Command::new(env!("CARGO_BIN_EXE_taxudromeio"))
        .args(["skill", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("Markdown")
    );
}

/// anchorbook: A-cli-skill#guide-examples
#[test]
fn published_shell_examples_parse_and_cover_every_mail_operation() {
    let guide = printed_guide();
    let mut operations = std::collections::BTreeSet::new();
    let mut root_example = false;
    let mut draft_example = false;
    let mut examples = 0;
    let mut json_examples = 0;
    for block in guide.split("```").skip(1).step_by(2) {
        let (language, code) = block.split_once('\n').unwrap();
        if language == "json" {
            let value: Value = serde_json::from_str(code).unwrap();
            assert!(value.is_object());
            json_examples += 1;
            continue;
        }
        if language != "sh" {
            continue;
        }
        // The examples use unquoted line continuations, which shells remove before
        // tokenization. shlex otherwise emits an empty argument for a standalone one.
        let shell_command = code.replace("\\\n", "");
        let mut args =
            shlex::split(&shell_command).expect("Valid shell quoting in the published guide");
        assert_eq!(args[0], "taxudromeio");
        // Redirecting the skill document is shell syntax; validate the executable arguments.
        if let Some(redirect) = args.iter().position(|arg| arg == ">") {
            assert_eq!(args[1], "skill");
            assert_eq!(args.len(), redirect + 2);
            args.truncate(redirect);
        }
        let parsed = Cli::try_parse_from(&args)
            .unwrap_or_else(|error| panic!("Unusable guide example:\n{code}\n{error}"));
        match parsed.command.unwrap() {
            ParsedCommand::Mail(command) => {
                let request = command.into_request();
                operations.insert(request.operation());
                assert_eq!(request.configuration(), "work");
                request.validate().unwrap();
                if let taxudromeio::mail::MailRequest::Draft(draft) = request {
                    assert_eq!(draft.subject, "Follow up");
                    assert_eq!(draft.body, "Hello, here is my draft.");
                    assert_eq!(draft.to, ["you@example.test", "other@example.test"]);
                    draft_example = true;
                }
            }
            ParsedCommand::Auth(auth) if auth.root.as_deref() == Some("INBOX/Sender: Work") => {
                assert_eq!(auth.id.as_deref(), Some("work"));
                assert!(auth.mobileconfig.is_some());
                root_example = true;
            }
            _ => {}
        }
        examples += 1;
    }
    assert_eq!(operations, Operation::ALL.into());
    assert!(root_example);
    assert!(draft_example);
    assert!(examples >= 15);
    assert_eq!(json_examples, 2);
}

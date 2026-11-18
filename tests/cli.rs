use std::process::Command;

/// anchorbook: A-auth-setup-help#setup-instructions
/// anchorbook: A-auth-setup-help#client-json
/// anchorbook: A-auth-setup-help#command-details
#[test]
fn installed_cli_help_provides_setup_and_usable_client_json() {
    let output = Command::new(env!("CARGO_BIN_EXE_taxudromeio"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("auth"));
    assert!(text.contains("serve"));
    assert!(text.contains("configurations"));
    assert!(text.contains("taxudromeio auth --id personal"));
    assert!(text.contains("macOS Keychain"));
    assert!(text.contains("mail_list_configurations"));

    // Parse the JSON from actual help output, then execute its command/args.
    // This catches invalid formatting and an unusable example executable path.
    let start = text.find("\n{").expect("MCP client JSON in --help") + 1;
    let end = start + text[start..].find("\n}").unwrap() + 2;
    let client: serde_json::Value = serde_json::from_str(&text[start..end]).unwrap();
    let server = &client["mcpServers"]["taxudromeio"];
    let command = server["command"].as_str().unwrap();
    assert!(std::path::Path::new(command).is_absolute());
    assert_eq!(
        std::fs::canonicalize(command).unwrap(),
        std::fs::canonicalize(env!("CARGO_BIN_EXE_taxudromeio")).unwrap()
    );
    assert_eq!(server["args"], serde_json::json!(["serve"]));
    assert!(server.get("env").is_none());
    let args = server["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg.as_str().unwrap())
        .collect::<Vec<_>>();
    let output = Command::new(command)
        .args(args)
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let serve_help = String::from_utf8(output.stdout).unwrap();
    assert!(serve_help.contains("--configuration personal --configuration work"));
    assert!(serve_help.contains("stdin/stdout"));

    let output = Command::new(command).arg("-h").output().unwrap();
    assert!(output.status.success());
    let short_help = String::from_utf8(output.stdout).unwrap();
    assert!(short_help.contains("Use --help for setup instructions"));
    assert!(!short_help.contains("mcpServers"));
    let output = Command::new(env!("CARGO_BIN_EXE_taxudromeio"))
        .args(["auth", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("--drafts-folder"));
    assert!(text.contains("--transport"));
    assert!(text.contains("--mobileconfig"));
    assert!(text.contains("--mail-account"));
    assert!(text.contains("starttls"));
    assert!(text.contains("provider-issued IMAP refresh token"));
    assert!(text.contains("INBOX does not include sibling folders"));
    assert!(text.contains("without changing mail"));
}

#[test]
fn mobileconfig_account_selection_requires_a_profile_path() {
    let output = Command::new(env!("CARGO_BIN_EXE_taxudromeio"))
        .args(["auth", "--mail-account", "2"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("--mobileconfig")
    );
}

#[test]
fn configuration_listing_does_not_need_keychain_and_missing_ids_fail() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "version = 1\n[[accounts]]\nid = 'work'\nhost = 'imap.example.com'\nroot = 'Projects'\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_taxudromeio"))
        .arg("--config")
        .arg(&path)
        .arg("configurations")
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["configurations"][0]["id"], "work");
    assert_eq!(value["configurations"][0]["root"], "Projects");
    assert!(
        value["configurations"][0]["operations"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("mail_save_draft"))
    );
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .contains("username")
    );
    let output = Command::new(env!("CARGO_BIN_EXE_taxudromeio"))
        .arg("--config")
        .arg(&path)
        .args(["serve", "--configuration", "missing"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("Unknown configuration")
    );
}

/// anchorbook: A-interface-shared#policy-boundary
#[test]
fn cli_refuses_disabled_operations_and_missing_ids_before_keychain() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "version = 1\n[[accounts]]\nid = 'archive'\nhost = 'imap.example.test'\nroot = 'INBOX'\n[accounts.policy]\nprofile = 'read_only'\n",
    )
    .unwrap();
    for (args, expected) in [
        (
            vec![
                "mail",
                "move",
                "--configuration",
                "archive",
                "--uids",
                "12",
                "--uid-validity",
                "7",
                "--destination",
                "Done",
            ],
            "disabled for this configuration",
        ),
        (
            vec!["mail", "folders", "--configuration", "missing"],
            "Unknown configuration",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_taxudromeio"))
            .arg("--config")
            .arg(&path)
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("Keychain"), "{error}");
    }
}

/// anchorbook: A-cli-agent-help#workflow-help
/// anchorbook: A-cli-agent-help#renamed-interface
#[test]
fn agent_help_teaches_workflow_and_every_example_parses() {
    use clap::Parser;
    use taxudromeio::{cli::Cli, config};

    let output = Command::new(env!("CARGO_BIN_EXE_taxudromeio"))
        .args(["--config", "/nonexistent/taxudromeio-settings", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).unwrap();
    for phrase in [
        "taxudromeio configurations",
        "JSON value to stdout",
        "--configuration ID",
        "next_before_uid",
        "UIDVALIDITY",
        "Reads never mark mail read",
        "untrusted data",
        "inspect mail before",
        "repeating creates another draft",
        "--unread=false",
        "exposes every saved configuration",
    ] {
        assert!(help.contains(phrase), "Missing guidance: {phrase}");
    }
    // These published examples deliberately use single-token argument values.
    // Parse what the executable actually printed, not a duplicate list of examples.
    let examples = help
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("taxudromeio mail "))
        .collect::<Vec<_>>();
    assert_eq!(examples.len(), 9);
    for example in examples {
        assert!(
            Cli::try_parse_from(example.split_whitespace()).is_ok(),
            "Unusable help example: {example}"
        );
    }
    for command in [
        "folders", "status", "list", "search", "read", "move", "copy", "flags", "draft",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_taxudromeio"))
            .args(["mail", command, "--help"])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("--configuration <CONFIGURATION>"), "{help}");
    }
    assert!(
        config::default_path()
            .unwrap()
            .ends_with("taxudromeio-mcp/config.toml")
    );
}

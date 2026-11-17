use std::process::Command;

#[test]
fn installed_cli_help_describes_auth_and_scoped_serving() {
    let output = Command::new(env!("CARGO_BIN_EXE_taxudromeio-mcp"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("auth"));
    assert!(text.contains("serve"));
    assert!(text.contains("configurations"));
    let output = Command::new(env!("CARGO_BIN_EXE_taxudromeio-mcp"))
        .args(["auth", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("--drafts-folder"));
    assert!(text.contains("--transport"));
    assert!(text.contains("starttls"));
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
    let output = Command::new(env!("CARGO_BIN_EXE_taxudromeio-mcp"))
        .arg("--config")
        .arg(&path)
        .arg("configurations")
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["accounts"][0]["policy"]["profile"], "triage");
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .contains("username")
    );
    let output = Command::new(env!("CARGO_BIN_EXE_taxudromeio-mcp"))
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

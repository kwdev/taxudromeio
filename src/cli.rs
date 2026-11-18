use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::Value;

use crate::{
    auth::AuthArgs,
    mail::{
        DraftArgs, FlagArgs, FolderArgs, ListArgs, MailRequest, ReadArgs, SearchArgs, TransferArgs,
    },
    service::MailService,
};

#[derive(Parser)]
#[command(
    name = "taxudromeio",
    version,
    about = "Scoped IMAP mail triage through CLI commands or a local MCP server",
    after_help = "Get started: taxudromeio auth\nDetailed setup and examples: taxudromeio skill\nMCP client JSON: taxudromeio mcp --json\nCommand arguments: taxudromeio <COMMAND> --help"
)]
pub struct Cli {
    /// Override the nonsecret configuration file (use the same path for auth and operations).
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Print a detailed Markdown usage guide for users and agents.
    Skill,
    /// Verify an IMAP login and store its login and credential in macOS Keychain.
    #[command(
        after_help = "Run in an interactive terminal; login and credentials stay in macOS Keychain.\nDetailed setup: taxudromeio skill"
    )]
    Auth(AuthArgs),
    /// Start the MCP server, or print client configuration with --json.
    #[command(
        after_help = "JSON uses this executable's absolute path and launches serve.\nPrinting JSON needs no saved accounts. Detailed setup: taxudromeio skill"
    )]
    Mcp {
        /// Print MCP client JSON instead of starting the server.
        #[arg(long)]
        json: bool,
        /// Expose only these IDs. Repeat to select several; omitted means all.
        #[arg(long = "configuration")]
        configurations: Vec<String>,
    },
    /// Start the MCP server over stdio (also the default command).
    #[command(
        after_help = "Communicates over stdin/stdout. All configurations are exposed unless selected.\nRestart after configuration changes. MCP client setup: taxudromeio skill"
    )]
    Serve {
        /// Expose only these IDs. Repeat to select several; omitted means all.
        #[arg(long = "configuration")]
        configurations: Vec<String>,
    },
    /// Print configuration IDs, roots and permitted operations as JSON without connecting.
    Configurations,
    /// Run one mail operation with CLI arguments and print its JSON result.
    #[command(
        subcommand,
        after_help = "Commands return JSON. Folder paths are relative to the selected root.\nDetailed workflow and examples: taxudromeio skill"
    )]
    Mail(MailCommand),
    /// Remove a configuration and its Keychain record.
    Remove { id: String },
}

pub const SKILL: &str = include_str!("../docs/skill.md");

/// Generate client launch settings without loading accounts or credentials.
pub fn mcp_client_config(config: Option<&Path>, configurations: &[String]) -> Result<Value> {
    let executable = std::env::current_exe().context("Could not locate this executable")?;
    let command = executable
        .to_str()
        .context("Executable path cannot be represented in JSON as UTF-8")?;
    let mut args = Vec::new();
    if let Some(config) = config {
        // Clients can launch from another directory, and the file need not exist yet.
        let path = std::path::absolute(config).context("Could not resolve the settings path")?;
        args.push("--config".to_owned());
        args.push(
            path.to_str()
                .context("Settings path cannot be represented in JSON as UTF-8")?
                .to_owned(),
        );
    }
    args.push("serve".to_owned());
    for id in configurations {
        args.push("--configuration".to_owned());
        args.push(id.clone());
    }
    Ok(serde_json::json!({
        "mcpServers": {
            "taxudromeio": {
                "command": command,
                "args": args
            }
        }
    }))
}

#[derive(Subcommand)]
pub enum MailCommand {
    /// List folders inside the selected root.
    Folders(FolderArgs),
    /// Read a folder's message count and UIDVALIDITY.
    Status(FolderArgs),
    /// List newest message headers and UIDs without marking mail read.
    List(ListArgs),
    /// Search with typed filters; results include UIDs and UIDVALIDITY.
    Search(SearchArgs),
    /// Read MIME text and attachment metadata without marking mail read.
    Read(ReadArgs),
    /// Move messages within the selected root (requires server UID MOVE support).
    Move(TransferArgs),
    /// Copy messages within the selected root.
    Copy(TransferArgs),
    /// Add or remove seen, flagged, answered or draft flags.
    Flags(FlagArgs),
    /// Save a new draft in IMAP; never send mail.
    Draft(DraftArgs),
}

impl MailCommand {
    pub fn into_request(self) -> MailRequest {
        match self {
            Self::Folders(args) => MailRequest::Folders(args),
            Self::Status(args) => MailRequest::Status(args),
            Self::List(args) => MailRequest::List(args),
            Self::Search(args) => MailRequest::Search(args),
            Self::Read(args) => MailRequest::Read(args),
            Self::Move(args) => MailRequest::Move(args),
            Self::Copy(args) => MailRequest::Copy(args),
            Self::Flags(args) => MailRequest::Flags(args),
            Self::Draft(args) => MailRequest::Draft(args),
        }
    }
}

pub async fn run_mail(command: MailCommand, service: &MailService) -> Result<Value> {
    service.execute(command.into_request()).await
}

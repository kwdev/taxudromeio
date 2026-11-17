use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use rmcp::{ServiceExt, transport::stdio};
use taxudromeio_mcp::{
    auth::{self, AuthArgs},
    config::{self, Config},
    credentials::Keychain,
    mail::ImapBackend,
    server::MailServer,
};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Override the nonsecret configuration file.
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Verify an IMAP login and store its login and credential in macOS Keychain.
    Auth(AuthArgs),
    /// Start the MCP server over stdio (also the default command).
    Serve {
        /// Expose only these IDs. Repeat this option to select several.
        #[arg(long = "configuration")]
        configurations: Vec<String>,
    },
    /// List configurations without opening Keychain or connecting to IMAP.
    Configurations,
    /// Remove a configuration and its Keychain record.
    Remove { id: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let path = cli.config.map(Ok).unwrap_or_else(config::default_path)?;
    match cli.command.unwrap_or(Command::Serve {
        configurations: vec![],
    }) {
        Command::Auth(args) => {
            tokio::task::spawn_blocking(move || auth::run(&path, args, &Keychain))
                .await
                .context("Authentication worker failed")??;
        }
        Command::Configurations => {
            let config = Config::load(&path)?;
            println!("{}", serde_json::to_string_pretty(&config)?);
        }
        Command::Remove { id } => {
            auth::remove(&path, &id, &Keychain)?;
            eprintln!("Removed configuration '{id}' and its Keychain record.");
        }
        Command::Serve { configurations } => {
            let mut config = Config::load(&path)?;
            for id in &configurations {
                config.account(id)?;
            }
            if !configurations.is_empty() {
                config
                    .accounts
                    .retain(|account| configurations.contains(&account.id));
            }
            let backend = Arc::new(ImapBackend::new(Arc::new(Keychain)));
            let service = MailServer::new(config, backend)?
                .serve(stdio())
                .await
                .context("MCP startup failed")?;
            service.waiting().await.context("MCP service failed")?;
        }
    }
    Ok(())
}

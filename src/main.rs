use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use rmcp::{ServiceExt, transport::stdio};
use taxudromeio::{
    auth,
    cli::{self, Cli, Command},
    config::{self, Config},
    credentials::Keychain,
    mail::ImapBackend,
    server::MailServer,
    service::{MailService, configurations},
};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    // Guidance and client settings are available before account setup.
    match &cli.command {
        Some(Command::Skill) => {
            print!("{}", cli::SKILL);
            return Ok(());
        }
        Some(Command::Mcp {
            json: true,
            configurations,
        }) => {
            let client = cli::mcp_client_config(cli.config.as_deref(), configurations)?;
            println!("{}", serde_json::to_string_pretty(&client)?);
            return Ok(());
        }
        _ => {}
    }
    let path = cli.config.map(Ok).unwrap_or_else(config::default_path)?;
    match cli.command.unwrap_or(Command::Serve {
        configurations: vec![],
    }) {
        Command::Skill | Command::Mcp { json: true, .. } => {
            unreachable!("Standalone output was handled before configuration lookup")
        }
        Command::Auth(args) => {
            tokio::task::spawn_blocking(move || auth::run(&path, args, &Keychain))
                .await
                .context("Authentication worker failed")??;
        }
        Command::Configurations => {
            let config = Config::load(&path)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&configurations(&config))?
            );
        }
        Command::Mail(command) => {
            let config = Config::load(&path)?;
            let backend = Arc::new(ImapBackend::new(Arc::new(Keychain)));
            let service = MailService::new(config, backend)?;
            let result = cli::run_mail(command, &service).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::Remove { id } => {
            auth::remove(&path, &id, &Keychain)?;
            eprintln!("Removed configuration '{id}' and its Keychain record.");
        }
        Command::Serve { configurations }
        | Command::Mcp {
            json: false,
            configurations,
        } => {
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

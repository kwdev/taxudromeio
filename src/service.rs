use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use tokio::sync::Semaphore;

use crate::{
    config::Config,
    mail::{MailBackend, MailRequest},
    policy::Operation,
};

/// The common authorization and execution boundary for CLI and MCP adapters.
#[derive(Clone)]
pub struct MailService {
    config: Arc<Config>,
    backend: Arc<dyn MailBackend>,
    permits: Arc<Semaphore>,
}

/// Nonsecret discovery is shared by MCP and `taxudromeio configurations`.
pub fn configurations(config: &Config) -> Value {
    json!({"configurations": config.accounts.iter().map(|a| json!({
        "id": a.id, "host": a.host, "port": a.port, "transport": a.transport,
        "root": a.root, "drafts_folder": a.drafts_folder,
        "operations": Operation::ALL.into_iter().filter(|op| a.policy.allows(*op))
            .map(Operation::tool_name).collect::<Vec<_>>(),
    })).collect::<Vec<_>>()})
}

impl MailService {
    pub fn new(config: Config, backend: Arc<dyn MailBackend>) -> Result<Self> {
        config.validate()?;
        ensure!(
            !config.accounts.is_empty(),
            "No configurations; run taxudromeio auth"
        );
        Ok(Self {
            config: Arc::new(config),
            backend,
            permits: Arc::new(Semaphore::new(4)),
        })
    }

    pub fn configurations(&self) -> Value {
        configurations(&self.config)
    }

    pub fn operations(&self) -> impl Iterator<Item = Operation> + '_ {
        Operation::ALL.into_iter().filter(|operation| {
            self.config
                .accounts
                .iter()
                .any(|account| account.policy.allows(*operation))
        })
    }

    pub async fn execute(&self, request: MailRequest) -> Result<Value> {
        let account = self.config.account(request.configuration())?.clone();
        // Both interfaces reject disabled operations before backend, Keychain or socket access.
        account.policy.require(request.operation())?;
        request.validate()?;
        let permit = self.permits.clone().acquire_owned().await?;
        let backend = self.backend.clone();
        tokio::task::spawn_blocking(move || {
            // Retain the permit until blocking I/O ends even if the caller cancels.
            let _permit = permit;
            backend.execute(&account, request)
        })
        .await
        .context("IMAP worker failed")?
    }
}

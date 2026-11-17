use std::{
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use imap::{Session, types::Flag};
use mailparse::MailHeaderMap;
use native_tls::{TlsConnector, TlsStream};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::mailbox_codec::{decode as decode_folder, encode as encode_folder};
use crate::{
    config::{Account, AuthType, Transport, safe_text},
    credentials::{CredentialStore, Credentials},
    oauth::CredentialManager,
    policy::Operation,
    scope::FolderScope,
};

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_MESSAGE_BYTES: u32 = 5 * 1024 * 1024;
const MAX_BODY_CHARS: usize = 40_000;
const MAX_UIDS: usize = 100;

fn root_folder() -> String {
    ".".into()
}
fn default_limit() -> usize {
    50
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FolderArgs {
    pub configuration: String,
    #[serde(default = "root_folder")]
    pub folder: String,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListArgs {
    pub configuration: String,
    #[serde(default = "root_folder")]
    pub folder: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Return older messages than this UID, for pagination.
    pub before_uid: Option<u32>,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchArgs {
    pub configuration: String,
    #[serde(default = "root_folder")]
    pub folder: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    pub before_uid: Option<u32>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub subject: Option<String>,
    pub text: Option<String>,
    pub unread: Option<bool>,
    pub flagged: Option<bool>,
    /// Inclusive IMAP internal date, YYYY-MM-DD.
    pub since: Option<String>,
    /// Exclusive IMAP internal date, YYYY-MM-DD.
    pub before: Option<String>,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadArgs {
    pub configuration: String,
    #[serde(default = "root_folder")]
    pub folder: String,
    pub uid: u32,
    pub uid_validity: u32,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransferArgs {
    pub configuration: String,
    #[serde(default = "root_folder")]
    pub folder: String,
    pub destination: String,
    pub uids: Vec<u32>,
    pub uid_validity: u32,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TriageFlag {
    Seen,
    Flagged,
    Answered,
    Draft,
}

impl TriageFlag {
    fn imap(self) -> &'static str {
        match self {
            Self::Seen => "\\Seen",
            Self::Flagged => "\\Flagged",
            Self::Answered => "\\Answered",
            Self::Draft => "\\Draft",
        }
    }
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FlagAction {
    Add,
    Remove,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlagArgs {
    pub configuration: String,
    #[serde(default = "root_folder")]
    pub folder: String,
    pub uids: Vec<u32>,
    pub uid_validity: u32,
    pub action: FlagAction,
    pub flags: Vec<TriageFlag>,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DraftArgs {
    pub configuration: String,
    /// Optional relative destination. Otherwise use this configuration's drafts_folder.
    pub folder: Option<String>,
    /// A bare email address; display names are not supported here.
    pub from: String,
    #[serde(default)]
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
}

#[derive(Clone)]
pub enum MailRequest {
    Folders(FolderArgs),
    Status(FolderArgs),
    List(ListArgs),
    Search(SearchArgs),
    Read(ReadArgs),
    Move(TransferArgs),
    Copy(TransferArgs),
    Flags(FlagArgs),
    Draft(DraftArgs),
}

impl MailRequest {
    pub fn parse(operation: Operation, value: Value) -> Result<Self> {
        Ok(match operation {
            Operation::ListFolders => Self::Folders(serde_json::from_value(value)?),
            Operation::FolderStatus => Self::Status(serde_json::from_value(value)?),
            Operation::ListMessages => Self::List(serde_json::from_value(value)?),
            Operation::SearchMessages => Self::Search(serde_json::from_value(value)?),
            Operation::ReadMessage => Self::Read(serde_json::from_value(value)?),
            Operation::MoveMessages => Self::Move(serde_json::from_value(value)?),
            Operation::CopyMessages => Self::Copy(serde_json::from_value(value)?),
            Operation::SetFlags => Self::Flags(serde_json::from_value(value)?),
            Operation::SaveDraft => Self::Draft(serde_json::from_value(value)?),
        })
    }

    pub fn configuration(&self) -> &str {
        match self {
            Self::Folders(a) | Self::Status(a) => &a.configuration,
            Self::List(a) => &a.configuration,
            Self::Search(a) => &a.configuration,
            Self::Read(a) => &a.configuration,
            Self::Move(a) | Self::Copy(a) => &a.configuration,
            Self::Flags(a) => &a.configuration,
            Self::Draft(a) => &a.configuration,
        }
    }

    pub fn operation(&self) -> Operation {
        match self {
            Self::Folders(_) => Operation::ListFolders,
            Self::Status(_) => Operation::FolderStatus,
            Self::List(_) => Operation::ListMessages,
            Self::Search(_) => Operation::SearchMessages,
            Self::Read(_) => Operation::ReadMessage,
            Self::Move(_) => Operation::MoveMessages,
            Self::Copy(_) => Operation::CopyMessages,
            Self::Flags(_) => Operation::SetFlags,
            Self::Draft(_) => Operation::SaveDraft,
        }
    }
}

pub trait MailBackend: Send + Sync {
    fn execute(&self, account: &Account, request: MailRequest) -> Result<Value>;
}

pub struct ImapBackend {
    credentials: CredentialManager,
}

impl ImapBackend {
    pub fn new(credentials: Arc<dyn CredentialStore>) -> Self {
        Self {
            credentials: CredentialManager::new(credentials),
        }
    }
}

type ImapSession = Session<TlsStream<TcpStream>>;

struct Connection<S: Read + Write> {
    session: Session<S>,
    scope: FolderScope,
}

struct Xoauth2<'a>(&'a Credentials);

impl imap::Authenticator for Xoauth2<'_> {
    type Response = String;

    fn process(&self, challenge: &[u8]) -> String {
        if !challenge.is_empty() {
            // XOAUTH2 error challenges require an empty response, not another token.
            return String::new();
        }
        format!(
            "user={}\x01auth=Bearer {}\x01\x01",
            self.0.username, self.0.secret
        )
    }
}

fn connect(
    account: &Account,
    credentials: &Credentials,
) -> Result<Connection<TlsStream<TcpStream>>> {
    let connector = TlsConnector::builder()
        .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
        .build()?;
    connect_with_connector(account, credentials, &connector)
}

#[derive(Debug)]
struct AuthenticationRejected;

impl std::fmt::Display for AuthenticationRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("IMAP authentication failed; check your app password or OAuth credentials")
    }
}

impl std::error::Error for AuthenticationRejected {}

fn connect_with_connector(
    account: &Account,
    credentials: &Credentials,
    connector: &TlsConnector,
) -> Result<Connection<TlsStream<TcpStream>>> {
    ensure!(
        safe_text(&credentials.username) && safe_text(&credentials.secret),
        "Invalid login or credential"
    );
    let deadline = Instant::now() + TIMEOUT;
    let mut tcp = None;
    let addresses = (account.host.as_str(), account.port)
        .to_socket_addrs()
        .context("Could not resolve IMAP host")?;
    for address in addresses {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        if let Ok(stream) = TcpStream::connect_timeout(&address, remaining) {
            tcp = Some(stream);
            break;
        }
    }
    let tcp = tcp.context("Could not connect to IMAP host within 30 seconds")?;
    tcp.set_read_timeout(Some(TIMEOUT))?;
    tcp.set_write_timeout(Some(TIMEOUT))?;
    let client = match account.transport {
        Transport::Tls => {
            let stream = connector
                .connect(&account.host, tcp)
                .context("IMAP TLS handshake failed")?;
            let mut client = imap::Client::new(stream);
            client.read_greeting().context("IMAP greeting failed")?;
            client
        }
        Transport::Starttls => {
            let mut client = imap::Client::new(tcp);
            client.read_greeting().context("IMAP greeting failed")?;
            // secure() requires STARTTLS OK and then verifies the certificate/hostname.
            // A rejected command or failed handshake drops the socket without login.
            client
                .secure(&account.host, connector)
                .context("Required IMAP STARTTLS upgrade failed")?
        }
    };
    let mut session = match account.auth_type {
        AuthType::Password => client.login(&credentials.username, &credentials.secret),
        AuthType::Oauth2 => client.authenticate("XOAUTH2", &Xoauth2(credentials)),
    }
    // Server authentication error text is deliberately suppressed; it can echo login data.
    .map_err(|_| anyhow::Error::new(AuthenticationRejected))?;
    let scope = discover_scope(&mut session, &account.root)?;
    Ok(Connection { session, scope })
}

fn discover_scope(session: &mut ImapSession, root: &str) -> Result<FolderScope> {
    let encoded = encode_folder(root);
    let names = session
        .list(Some(""), Some(&encoded))
        .context("Could not discover root folder")?;
    let name = names
        .iter()
        .find(|name| {
            name.name() == encoded
                || (root.eq_ignore_ascii_case("INBOX") && name.name().eq_ignore_ascii_case("INBOX"))
        })
        .context("The configured root folder does not exist")?;
    let delimiter = match name.delimiter() {
        None => None,
        Some(d) if d.chars().count() == 1 => d.chars().next(),
        Some(_) => bail!("Server returned an invalid hierarchy delimiter"),
    };
    FolderScope::new(decode_folder(name.name())?, delimiter)
}

pub fn verify(account: &Account, credentials: &Credentials) -> Result<()> {
    let mut connection = connect(account, credentials)?;
    // A root can be a nonselectable namespace containing selectable descendants.
    if let Some(folder) = &account.drafts_folder {
        let folder = connection.scope.resolve(folder)?;
        connection
            .session
            .examine(encode_folder(&folder))
            .context("Drafts folder does not exist or is not selectable")?;
    }
    Ok(())
}

impl MailBackend for ImapBackend {
    fn execute(&self, account: &Account, request: MailRequest) -> Result<Value> {
        account.policy.require(request.operation())?;
        ensure!(
            request.configuration() == account.id,
            "Configuration mismatch"
        );
        let mut connection = authenticated(account, &self.credentials, |credentials| {
            connect(account, credentials)
        })?;
        connection.execute(account, request)
    }
}

fn authenticated<T>(
    account: &Account,
    manager: &CredentialManager,
    mut connect: impl FnMut(&Credentials) -> Result<T>,
) -> Result<T> {
    let credentials = manager.fresh(account)?;
    match connect(&credentials) {
        Err(error)
            if error.is::<AuthenticationRejected>()
                && matches!(account.auth_type, AuthType::Oauth2) =>
        {
            let refreshed = manager.after_rejection(account, &credentials)?;
            connect(&refreshed)
        }
        other => other,
    }
}

fn uid_set(uids: &[u32]) -> Result<String> {
    ensure!(
        !uids.is_empty() && uids.len() <= MAX_UIDS,
        "Supply 1–100 UIDs"
    );
    ensure!(
        uids.iter().all(|uid| *uid != 0),
        "UIDs must be positive integers"
    );
    let unique: std::collections::BTreeSet<_> = uids.iter().copied().collect();
    Ok(unique
        .into_iter()
        .map(|uid| uid.to_string())
        .collect::<Vec<_>>()
        .join(","))
}

fn require_uid_validity(actual: Option<u32>, expected: u32) -> Result<()> {
    ensure!(
        expected != 0 && actual == Some(expected),
        "UIDVALIDITY changed or is unavailable; list this folder again"
    );
    Ok(())
}

fn quoted(value: &str) -> Result<String> {
    ensure!(
        safe_text(value) && value.len() <= 1000,
        "Search fields must be nonempty, at most 1000 bytes, and have no control characters"
    );
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

fn search_query(args: &SearchArgs) -> Result<String> {
    let mut terms = vec!["ALL".to_string()];
    for (key, value) in [
        ("FROM", &args.from),
        ("TO", &args.to),
        ("SUBJECT", &args.subject),
        ("TEXT", &args.text),
    ] {
        if let Some(value) = value {
            terms.push(format!("{key} {}", quoted(value)?));
        }
    }
    if let Some(unread) = args.unread {
        terms.push(if unread { "UNSEEN" } else { "SEEN" }.into());
    }
    if let Some(flagged) = args.flagged {
        terms.push(if flagged { "FLAGGED" } else { "UNFLAGGED" }.into());
    }
    for (key, value) in [("SINCE", &args.since), ("BEFORE", &args.before)] {
        if let Some(value) = value {
            ensure!(value.len() == 10, "Date must be YYYY-MM-DD");
            let date = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .context("Invalid date; use YYYY-MM-DD")?;
            terms.push(format!("{key} {}", date.format("%d-%b-%Y")));
        }
    }
    if let Some(before) = args.before_uid {
        ensure!(before > 1, "before_uid must be greater than 1");
        terms.push(format!("UID 1:{}", before - 1));
    }
    Ok(terms.join(" "))
}

impl<S: Read + Write> Connection<S> {
    fn select(
        &mut self,
        relative: &str,
        write: bool,
        validity: Option<u32>,
    ) -> Result<imap::types::Mailbox> {
        let folder = self.scope.resolve(relative)?;
        let encoded = encode_folder(&folder);
        let mailbox = if write {
            self.session.select(encoded)?
        } else {
            self.session.examine(encoded)?
        };
        if let Some(expected) = validity {
            require_uid_validity(mailbox.uid_validity, expected)?;
        }
        Ok(mailbox)
    }

    fn execute(&mut self, account: &Account, request: MailRequest) -> Result<Value> {
        match request {
            MailRequest::Folders(args) => {
                let parent = self.scope.resolve(&args.folder)?;
                let names = self.session.list(Some(""), Some("*"))?;
                let mut folders = Vec::new();
                for name in names.iter() {
                    let absolute = decode_folder(name.name())?;
                    if let Some(relative) = self.scope.relative(&absolute) {
                        let within_parent = absolute == parent
                            || self
                                .scope
                                .delimiter()
                                .is_some_and(|d| absolute.starts_with(&format!("{parent}{d}")));
                        if within_parent {
                            folders.push(json!({
                                "folder": relative,
                                "attributes": name.attributes().iter().map(|a| format!("{a:?}")).collect::<Vec<_>>(),
                            }));
                        }
                    }
                }
                Ok(
                    json!({"configuration": account.id, "delimiter": self.scope.delimiter(), "folders": folders}),
                )
            }
            MailRequest::Status(args) => {
                let mailbox = self.select(&args.folder, false, None)?;
                Ok(
                    json!({"folder": args.folder, "messages": mailbox.exists, "uid_validity": mailbox.uid_validity, "uid_next": mailbox.uid_next}),
                )
            }
            MailRequest::List(args) => {
                let query = SearchArgs {
                    configuration: args.configuration,
                    folder: args.folder,
                    limit: args.limit,
                    before_uid: args.before_uid,
                    from: None,
                    to: None,
                    subject: None,
                    text: None,
                    unread: None,
                    flagged: None,
                    since: None,
                    before: None,
                };
                self.search(query)
            }
            MailRequest::Search(args) => self.search(args),
            MailRequest::Read(args) => {
                let set = uid_set(&[args.uid])?;
                self.select(&args.folder, false, Some(args.uid_validity))?;
                let size = {
                    let fetches = self.session.uid_fetch(&set, "(UID RFC822.SIZE)")?;
                    fetches
                        .iter()
                        .find(|f| f.uid == Some(args.uid))
                        .and_then(|f| f.size)
                        .context("Message not found or size unavailable")?
                };
                ensure!(
                    size <= MAX_MESSAGE_BYTES,
                    "Message exceeds the 5 MiB read limit"
                );
                let fetches = self.session.uid_fetch(&set, "(UID FLAGS BODY.PEEK[])")?;
                let fetch = fetches
                    .iter()
                    .find(|f| f.uid == Some(args.uid))
                    .context("Message no longer exists")?;
                let bytes = fetch.body().context("Server returned no message body")?;
                ensure!(
                    bytes.len() <= MAX_MESSAGE_BYTES as usize,
                    "Message exceeds the 5 MiB read limit"
                );
                let parsed = mailparse::parse_mail(bytes)?;
                let mut parts = Vec::new();
                let mut remaining = MAX_BODY_CHARS;
                collect_parts(&parsed, &mut parts, &mut remaining)?;
                Ok(json!({
                    "folder": args.folder, "uid": args.uid, "uid_validity": args.uid_validity,
                    "headers": message_headers(&parsed.headers),
                    "flags": fetch.flags().iter().map(ToString::to_string).collect::<Vec<_>>(),
                    "parts": parts,
                }))
            }
            MailRequest::Move(args) => self.transfer(args, true),
            MailRequest::Copy(args) => self.transfer(args, false),
            MailRequest::Flags(args) => {
                let set = uid_set(&args.uids)?;
                ensure!(
                    !args.flags.is_empty() && args.flags.len() <= 4,
                    "Supply 1–4 supported flags"
                );
                self.select(&args.folder, true, Some(args.uid_validity))?;
                let action = match args.action {
                    FlagAction::Add => "+FLAGS.SILENT",
                    FlagAction::Remove => "-FLAGS.SILENT",
                };
                let flags = args
                    .flags
                    .into_iter()
                    .map(TriageFlag::imap)
                    .collect::<Vec<_>>()
                    .join(" ");
                self.session.uid_store(set, format!("{action} ({flags})"))?;
                Ok(
                    json!({"updated_uids": args.uids, "folder": args.folder, "uid_validity": args.uid_validity}),
                )
            }
            MailRequest::Draft(args) => {
                let folder = args
                    .folder
                    .as_ref()
                    .or(account.drafts_folder.as_ref())
                    .context(
                        "Set drafts_folder in this configuration or supply a relative folder",
                    )?;
                let destination = self.scope.resolve(folder)?;
                let message = build_draft(&args)?;
                self.session.append_with_flags(
                    encode_folder(&destination),
                    message,
                    &[Flag::Draft, Flag::Seen],
                )?;
                Ok(json!({"saved": true, "folder": folder, "sent": false}))
            }
        }
    }

    fn transfer(&mut self, args: TransferArgs, move_mail: bool) -> Result<Value> {
        let set = uid_set(&args.uids)?;
        let destination = self.scope.resolve(&args.destination)?;
        let source = self.scope.resolve(&args.folder)?;
        ensure!(
            source != destination,
            "Source and destination folders must differ"
        );
        self.select(&args.folder, true, Some(args.uid_validity))?;
        if move_mail {
            let supports_move = self.session.capabilities()?.has_str("MOVE");
            ensure!(
                supports_move,
                "Server does not support UID MOVE; use copy or another mail client"
            );
            self.session.uid_mv(set, encode_folder(&destination))?;
        } else {
            self.session.uid_copy(set, encode_folder(&destination))?;
        }
        // Source UIDs are not identifiers in the destination. Relist it before acting.
        Ok(json!({
            "source_uids": args.uids, "source": args.folder, "destination": args.destination,
            "moved": move_mail, "destination_uids": null,
            "next_step": "List the destination to get its UIDs and UIDVALIDITY",
        }))
    }

    fn search(&mut self, args: SearchArgs) -> Result<Value> {
        ensure!(
            (1..=100).contains(&args.limit),
            "limit must be between 1 and 100"
        );
        let query = search_query(&args)?;
        let mailbox = self.select(&args.folder, false, None)?;
        let validity = mailbox
            .uid_validity
            .context("Server did not provide UIDVALIDITY")?;
        let mut uids = self
            .session
            .uid_search(query)?
            .into_iter()
            .collect::<Vec<_>>();
        uids.sort_unstable_by(|a, b| b.cmp(a));
        let more = uids.len() > args.limit;
        uids.truncate(args.limit);
        let mut messages = Vec::new();
        if !uids.is_empty() {
            let fetches = self.session.uid_fetch(
                uid_set(&uids)?,
                "(UID FLAGS RFC822.SIZE BODY.PEEK[HEADER]<0.16384>)",
            )?;
            for fetch in fetches.iter() {
                let Some(uid) = fetch.uid else { continue };
                // Ignore unsolicited FETCH responses for messages outside the requested page.
                if !uids.contains(&uid) {
                    continue;
                }
                let headers = fetch
                    .header()
                    .map(mailparse::parse_headers)
                    .transpose()?
                    .map(|(h, _)| h)
                    .unwrap_or_default();
                messages.push(json!({
                    "uid": uid, "size": fetch.size,
                    "flags": fetch.flags().iter().map(ToString::to_string).collect::<Vec<_>>(),
                    "headers": message_headers(&headers),
                }));
            }
            messages.sort_by_key(|message| std::cmp::Reverse(message["uid"].as_u64().unwrap_or(0)));
        }
        Ok(json!({
            "folder": args.folder, "uid_validity": validity, "messages": messages,
            "more": more, "next_before_uid": if more { uids.last().copied() } else { None },
        }))
    }
}

fn message_headers(headers: &[mailparse::MailHeader<'_>]) -> Value {
    json!({
        "from": headers.get_first_value("From"), "to": headers.get_first_value("To"),
        "subject": headers.get_first_value("Subject"), "date": headers.get_first_value("Date"),
        "message_id": headers.get_first_value("Message-ID"),
        "in_reply_to": headers.get_first_value("In-Reply-To"),
    })
}

fn collect_parts(
    mail: &mailparse::ParsedMail<'_>,
    parts: &mut Vec<Value>,
    remaining: &mut usize,
) -> Result<()> {
    if parts.len() >= 100 {
        return Ok(());
    }
    let disposition = mail.get_content_disposition();
    let filename = disposition
        .params
        .get("filename")
        .or(mail.ctype.params.get("name"));
    if mail.subparts.is_empty() {
        if filename.is_none() && matches!(mail.ctype.mimetype.as_str(), "text/plain" | "text/html")
        {
            let body = mail.get_body()?;
            let truncated = body.chars().count() > *remaining;
            let text = body.chars().take(*remaining).collect::<String>();
            *remaining = remaining.saturating_sub(text.chars().count());
            parts.push(
                json!({"content_type": mail.ctype.mimetype, "text": text, "truncated": truncated}),
            );
        } else {
            parts.push(json!({"content_type": mail.ctype.mimetype, "filename": filename, "attachment": true}));
        }
    } else {
        for child in &mail.subparts {
            collect_parts(child, parts, remaining)?;
        }
    }
    Ok(())
}

fn address(value: &str) -> Result<&str> {
    ensure!(
        safe_text(value) && value.is_ascii() && value.len() <= 254,
        "Draft addresses must be bare ASCII email addresses"
    );
    let parsed = mailparse::addrparse(value)?;
    ensure!(parsed.len() == 1, "Each address must name one mailbox");
    match &parsed[0] {
        mailparse::MailAddr::Single(single)
            if single.display_name.is_none() && single.addr == value && value.contains('@') =>
        {
            Ok(value)
        }
        _ => bail!("Use a bare email address without a display name"),
    }
}

fn build_draft(args: &DraftArgs) -> Result<Vec<u8>> {
    address(&args.from)?;
    ensure!(
        !args.subject.chars().any(char::is_control) && args.subject.len() <= 1000,
        "Invalid draft subject"
    );
    ensure!(
        args.body.len() <= 1024 * 1024,
        "Draft body must be at most 1 MiB"
    );
    ensure!(
        args.to.len() + args.cc.len() + args.bcc.len() <= 100,
        "At most 100 draft recipients"
    );
    let mut message = format!("From: {}\r\n", args.from);
    for (header, recipients) in [("To", &args.to), ("Cc", &args.cc), ("Bcc", &args.bcc)] {
        for recipient in recipients {
            address(recipient)?;
        }
        if !recipients.is_empty() {
            message.push_str(&format!("{header}: {}\r\n", recipients.join(",\r\n ")));
        }
    }
    if let Some(reply) = &args.in_reply_to {
        ensure!(
            safe_text(reply)
                && reply.is_ascii()
                && reply.starts_with('<')
                && reply.ends_with('>')
                && !reply.chars().any(char::is_whitespace)
                && reply.len() <= 980,
            "Invalid In-Reply-To message ID"
        );
        message.push_str(&format!("In-Reply-To: {reply}\r\n"));
    }
    // Split encoded words at UTF-8 character boundaries, respecting RFC 2047's 75-byte cap.
    let mut chunk = String::new();
    let mut words = Vec::new();
    for ch in args.subject.chars() {
        if chunk.len() + ch.len_utf8() > 42 {
            words.push(format!("=?UTF-8?B?{}?=", STANDARD.encode(&chunk)));
            chunk.clear();
        }
        chunk.push(ch);
    }
    if !chunk.is_empty() {
        words.push(format!("=?UTF-8?B?{}?=", STANDARD.encode(chunk)));
    }
    message.push_str(&format!(
        "Subject: {}\r\nDate: {}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n",
        words.join("\r\n "), chrono::Utc::now().to_rfc2822(),
    ));
    let body = args
        .body
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n");
    let encoded = STANDARD.encode(body);
    for line in encoded.as_bytes().chunks(76) {
        message.push_str(std::str::from_utf8(line)?);
        message.push_str("\r\n");
    }
    Ok(message.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    mod authentication {
        use super::*;
        use crate::test_support::{self as fixture, MemoryStore};
        use std::{
            net::TcpListener,
            sync::atomic::{AtomicUsize, Ordering},
            thread,
        };

        /// anchorbook: A-auth-encrypted#upgrade-before-login
        #[test]
        fn starttls_upgrades_before_login_and_has_no_second_greeting() {
            for auth_type in [AuthType::Password, AuthType::Oauth2] {
                let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
                let mut account = fixture::account();
                account.port = listener.local_addr().unwrap().port();
                account.auth_type = auth_type;
                let oauth = matches!(account.auth_type, AuthType::Oauth2);
                let acceptor = fixture::acceptor();
                let server = thread::spawn(move || {
                    let (mut tcp, _) = listener.accept().unwrap();
                    tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    fixture::reply(&mut tcp, "* OK IMAP4rev1 STARTTLS fixture\r\n");
                    assert_eq!(fixture::line(&mut tcp), "a1 STARTTLS\r\n");
                    fixture::reply(&mut tcp, "a1 OK Begin TLS\r\n");
                    let mut tls = acceptor.accept(tcp).unwrap();
                    let login = fixture::line(&mut tls);
                    if oauth {
                        assert_eq!(login, "a1 AUTHENTICATE XOAUTH2\r\n");
                        fixture::reply(&mut tls, "+ \r\n");
                        let response = fixture::line(&mut tls);
                        assert_eq!(
                            STANDARD.decode(response.trim()).unwrap(),
                            b"user=fixture@example.test\x01auth=Bearer old-access\x01\x01"
                        );
                    } else {
                        assert_eq!(
                            login,
                            "a1 LOGIN \"fixture@example.test\" \"old-access\"\r\n"
                        );
                    }
                    fixture::reply(&mut tls, "a1 OK Authenticated\r\n");
                    assert_eq!(fixture::line(&mut tls), "a2 LIST \"\" INBOX\r\n");
                    fixture::reply(&mut tls, "* LIST () \"/\" \"INBOX\"\r\na2 OK Listed\r\n");
                    let mut extra = Vec::new();
                    let _ = tls.read_to_end(&mut extra);
                    assert!(extra.is_empty());
                });
                drop(
                    connect_with_connector(
                        &account,
                        &fixture::credentials(),
                        &fixture::connector(),
                    )
                    .unwrap(),
                );
                server.join().unwrap();
            }
        }

        /// anchorbook: A-auth-encrypted#no-downgrade
        #[test]
        fn rejected_or_unsupported_starttls_never_sends_credentials() {
            for response in ["NO TLS unavailable", "BAD Unknown command"] {
                let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
                let mut account = fixture::account();
                account.port = listener.local_addr().unwrap().port();
                let response = response.to_string();
                let server = thread::spawn(move || {
                    let (mut tcp, _) = listener.accept().unwrap();
                    tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    fixture::reply(&mut tcp, "* OK IMAP4rev1 fixture\r\n");
                    assert_eq!(fixture::line(&mut tcp), "a1 STARTTLS\r\n");
                    fixture::reply(&mut tcp, &format!("a1 {response}\r\n"));
                    let mut extra = Vec::new();
                    tcp.read_to_end(&mut extra).unwrap();
                    assert!(extra.is_empty());
                });
                assert!(
                    connect_with_connector(
                        &account,
                        &fixture::credentials(),
                        &fixture::connector()
                    )
                    .is_err()
                );
                server.join().unwrap();
            }
        }

        /// anchorbook: A-auth-encrypted#certificate-verification
        #[test]
        fn starttls_rejects_untrusted_certificates_and_hostname_mismatch_before_login() {
            for mismatch in [false, true] {
                let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
                let mut account = fixture::account();
                account.port = listener.local_addr().unwrap().port();
                if mismatch {
                    account.host = "127.0.0.1".into();
                }
                let connector = if mismatch {
                    fixture::connector()
                } else {
                    TlsConnector::new().unwrap()
                };
                let acceptor = fixture::acceptor();
                let server = thread::spawn(move || {
                    let (mut tcp, _) = listener.accept().unwrap();
                    tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    fixture::reply(&mut tcp, "* OK IMAP4rev1 fixture\r\n");
                    assert_eq!(fixture::line(&mut tcp), "a1 STARTTLS\r\n");
                    fixture::reply(&mut tcp, "a1 OK Begin TLS\r\n");
                    if let Ok(mut tls) = acceptor.accept(tcp) {
                        let mut bytes = Vec::new();
                        let _ = tls.read_to_end(&mut bytes);
                        assert!(
                            bytes.is_empty(),
                            "No IMAP login may follow a failed certificate check"
                        );
                    }
                });
                assert!(
                    connect_with_connector(&account, &fixture::credentials(), &connector).is_err()
                );
                server.join().unwrap();
            }
        }

        /// anchorbook: A-oauth-renewable#authentication-retry
        #[test]
        fn rejected_access_token_refreshes_once_and_only_retries_login() {
            let store = Arc::new(MemoryStore::new(fixture::credentials()));
            let refreshed = Arc::new(AtomicUsize::new(0));
            let count = refreshed.clone();
            let manager = CredentialManager::with_refresher(
                store.clone(),
                Arc::new(move |c| {
                    count.fetch_add(1, Ordering::SeqCst);
                    c.secret = "new-access".into();
                    c.oauth.as_mut().unwrap().refresh_token = "new-refresh".into();
                    Ok(())
                }),
            );
            let mut connections = 0;
            let result = authenticated(&fixture::account(), &manager, |c| {
                connections += 1;
                if connections == 1 {
                    assert_eq!(c.secret, "old-access");
                    Err(anyhow::Error::new(AuthenticationRejected))
                } else {
                    // Persistence must precede the new connection.
                    assert_eq!(store.record.lock().unwrap().secret, "new-access");
                    assert_eq!(c.secret, "new-access");
                    Ok("connected")
                }
            })
            .unwrap();
            assert_eq!(result, "connected");
            assert_eq!(connections, 2);
            assert_eq!(refreshed.load(Ordering::SeqCst), 1);
            let mut attempts = 0;
            let result: Result<()> = authenticated(&fixture::account(), &manager, |_| {
                attempts += 1;
                Err(anyhow::Error::new(AuthenticationRejected))
            });
            assert!(result.is_err());
            assert_eq!(attempts, 2);
            assert_eq!(refreshed.load(Ordering::SeqCst), 2);
            attempts = 0;
            let result: Result<()> = authenticated(&fixture::account(), &manager, |_| {
                attempts += 1;
                anyhow::bail!("TLS or root failure")
            });
            assert!(result.is_err());
            assert_eq!(attempts, 1);
            assert_eq!(refreshed.load(Ordering::SeqCst), 2);
        }

        /// anchorbook: A-oauth-renewable#failure-safety
        #[test]
        fn refresh_failure_stops_before_imap_connection_or_mutation() {
            for provider_failure in [false, true] {
                let mut credentials = fixture::credentials();
                credentials.oauth.as_mut().unwrap().expires_at = Some(1);
                let mut store = MemoryStore::new(credentials);
                store.fail_save = !provider_failure;
                let manager = CredentialManager::with_refresher(
                    Arc::new(store),
                    Arc::new(move |c| {
                        if provider_failure {
                            anyhow::bail!("Fixture refresh failed");
                        }
                        c.secret = "new-access".into();
                        Ok(())
                    }),
                );
                let mut connections = 0;
                let result: Result<()> = authenticated(&fixture::account(), &manager, |_| {
                    connections += 1;
                    Ok(())
                });
                assert!(result.is_err());
                assert_eq!(connections, 0);
            }
        }
    }

    /// anchorbook: A-message-stable#stale-identifiers
    #[test]
    fn stale_validity_and_invalid_uid_sets_fail() {
        assert!(require_uid_validity(Some(7), 7).is_ok());
        for (actual, expected) in [(Some(8), 7), (None, 7), (Some(0), 0)] {
            assert!(require_uid_validity(actual, expected).is_err());
        }
        for uids in [vec![], vec![0], vec![1; 101]] {
            assert!(uid_set(&uids).is_err());
        }
        assert_eq!(uid_set(&[9, 2, 9]).unwrap(), "2,9");
    }

    /// anchorbook: A-policy-triage#draft-only
    #[test]
    fn draft_is_valid_mime_and_header_injection_is_rejected() {
        let mut draft = DraftArgs {
            configuration: "work".into(),
            folder: None,
            from: "me@example.com".into(),
            to: vec!["you@example.com".into()],
            cc: vec![],
            bcc: vec![],
            subject: "Δοκιμή 🦀".repeat(20),
            body: "Hello\nκόσμε".into(),
            in_reply_to: None,
        };
        let raw = build_draft(&draft).unwrap();
        let parsed = mailparse::parse_mail(&raw).unwrap();
        assert_eq!(
            parsed.headers.get_first_value("Subject").unwrap(),
            draft.subject
        );
        assert_eq!(parsed.get_body().unwrap(), "Hello\r\nκόσμε");
        draft.to = vec!["you@example.com\r\nBcc: evil@example.com".into()];
        assert!(build_draft(&draft).is_err());
        draft.to.clear();
        draft.subject = "Hi\r\nX-Foo: bar".into();
        assert!(build_draft(&draft).is_err());
    }

    #[test]
    fn searches_quote_special_characters_and_reject_commands() {
        let mut args: SearchArgs =
            serde_json::from_value(json!({"configuration": "work", "subject": "a\"b\\c"})).unwrap();
        assert_eq!(search_query(&args).unwrap(), "ALL SUBJECT \"a\\\"b\\\\c\"");
        args.subject = Some("x\r\nUID STORE 1 +FLAGS (\\Deleted)".into());
        assert!(search_query(&args).is_err());
        args.subject = None;
        args.since = Some("2026-02-30".into());
        assert!(search_query(&args).is_err());
        assert!(serde_json::from_value::<FlagArgs>(json!({
            "configuration": "work", "uids": [1], "uid_validity": 7, "action": "add", "flags": ["deleted"]
        })).is_err());
    }

    #[cfg(unix)]
    mod wire {
        use super::*;
        use crate::{config::AuthType, policy::Policy};
        use std::{
            io::{BufRead, BufReader},
            os::unix::net::UnixStream,
            thread,
        };

        fn account() -> Account {
            Account {
                id: "work".into(),
                host: "imap.example.com".into(),
                port: 993,
                transport: Transport::Tls,
                root: "Projects".into(),
                drafts_folder: Some("Drafts".into()),
                auth_type: AuthType::Password,
                policy: Policy::default(),
            }
        }

        fn fixture(
            script: Vec<(String, String)>,
        ) -> (Connection<UnixStream>, thread::JoinHandle<()>) {
            let (client, server) = UnixStream::pair().unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            server
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let handle = thread::spawn(move || {
                let mut writer = server.try_clone().unwrap();
                let mut reader = BufReader::new(server);
                writer.write_all(b"* OK fixture ready\r\n").unwrap();
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let (tag, command) = line.trim_end().split_once(' ').unwrap();
                assert_eq!(command, "LOGIN \"fixture\" \"fixture\"");
                writer
                    .write_all(format!("{tag} OK logged in\r\n").as_bytes())
                    .unwrap();
                for (expected, response) in script {
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                    let (tag, command) = line.trim_end().split_once(' ').unwrap();
                    assert_eq!(command, expected);
                    writer
                        .write_all(response.replace("{tag}", tag).as_bytes())
                        .unwrap();
                }
                line.clear();
                assert_eq!(
                    reader.read_line(&mut line).unwrap(),
                    0,
                    "Unexpected IMAP command: {line}"
                );
            });
            let mut client = imap::Client::new(client);
            client.read_greeting().unwrap();
            let session = client
                .login("fixture", "fixture")
                .map_err(|(e, _)| e)
                .unwrap();
            (
                Connection {
                    session,
                    scope: FolderScope::new("Projects".into(), Some('/')).unwrap(),
                },
                handle,
            )
        }

        fn examine(validity: u32) -> String {
            format!(
                "* FLAGS (\\Seen \\Flagged)\r\n* 1 EXISTS\r\n* 0 RECENT\r\n* OK [UIDVALIDITY {validity}] validity\r\n{{tag}} OK [READ-ONLY] selected\r\n"
            )
        }

        /// anchorbook: A-folder-confined#wire-boundaries
        #[test]
        fn listed_siblings_are_filtered_and_invalid_destinations_send_no_commands() {
            let script = vec![(
                "LIST \"\" *".into(),
                "* LIST () \"/\" \"Projects\"\r\n* LIST () \"/\" \"Projects/Done\"\r\n* LIST () \"/\" \"ProjectsOther/Secret\"\r\n* LIST () \"/\" \"INBOX\"\r\n{tag} OK listed\r\n".into(),
            )];
            let (mut connection, handle) = fixture(script);
            let result = connection
                .execute(
                    &account(),
                    MailRequest::Folders(FolderArgs {
                        configuration: "work".into(),
                        folder: ".".into(),
                    }),
                )
                .unwrap();
            assert_eq!(result["folders"].as_array().unwrap().len(), 2);
            assert_eq!(result["folders"][1]["folder"], "Done");
            let move_request = MailRequest::Move(TransferArgs {
                configuration: "work".into(),
                folder: ".".into(),
                destination: "../INBOX".into(),
                uids: vec![12],
                uid_validity: 7,
            });
            assert!(connection.execute(&account(), move_request).is_err());
            let draft: DraftArgs = serde_json::from_value(json!({
                "configuration": "work", "folder": "/INBOX", "from": "me@example.com",
                "subject": "Draft", "body": "Text",
            }))
            .unwrap();
            assert!(
                connection
                    .execute(&account(), MailRequest::Draft(draft))
                    .is_err()
            );
            drop(connection);
            handle.join().unwrap();
        }

        /// anchorbook: A-message-stable#wire-safety
        #[test]
        fn reading_uses_examine_and_body_peek() {
            let message = "From: alice@example.com\r\nSubject: Hello\r\nContent-Type: text/plain\r\n\r\nMessage body";
            let script = vec![
                ("EXAMINE \"Projects\"".into(), examine(7)),
                (
                    "UID FETCH 12 (UID RFC822.SIZE)".into(),
                    format!(
                        "* 1 FETCH (UID 12 RFC822.SIZE {})\r\n{{tag}} OK fetched\r\n",
                        message.len()
                    ),
                ),
                (
                    "UID FETCH 12 (UID FLAGS BODY.PEEK[])".into(),
                    format!(
                        "* 1 FETCH (UID 12 FLAGS () BODY[] {{{}}}\r\n{message})\r\n{{tag}} OK fetched\r\n",
                        message.len()
                    ),
                ),
            ];
            let (mut connection, handle) = fixture(script);
            let result = connection
                .execute(
                    &account(),
                    MailRequest::Read(ReadArgs {
                        configuration: "work".into(),
                        folder: ".".into(),
                        uid: 12,
                        uid_validity: 7,
                    }),
                )
                .unwrap();
            assert_eq!(result["parts"][0]["text"], "Message body");
            assert_eq!(result["headers"]["subject"], "Hello");
            assert!(result["flags"].as_array().unwrap().is_empty());
            drop(connection);
            handle.join().unwrap();
        }

        /// anchorbook: A-message-stable#wire-safety
        #[test]
        fn listing_returns_decoded_headers_with_uid_pagination() {
            let header = "From: alice@example.com\r\nSubject: Hello\r\n\r\n";
            let script = vec![
                ("EXAMINE \"Projects\"".into(), examine(7)),
                (
                    "UID SEARCH ALL".into(),
                    "* SEARCH 5 12\r\n{tag} OK search\r\n".into(),
                ),
                (
                    "UID FETCH 12 (UID FLAGS RFC822.SIZE BODY.PEEK[HEADER]<0.16384>)".into(),
                    format!(
                        "* 1 FETCH (UID 12 FLAGS (\\Flagged) RFC822.SIZE 100 BODY[HEADER]<0> {{{}}}\r\n{header})\r\n{{tag}} OK fetched\r\n",
                        header.len()
                    ),
                ),
            ];
            let (mut connection, handle) = fixture(script);
            let result = connection
                .execute(
                    &account(),
                    MailRequest::List(ListArgs {
                        configuration: "work".into(),
                        folder: ".".into(),
                        limit: 1,
                        before_uid: None,
                    }),
                )
                .unwrap();
            assert_eq!(result["uid_validity"], 7);
            assert_eq!(result["messages"][0]["uid"], 12);
            assert_eq!(result["messages"][0]["headers"]["subject"], "Hello");
            assert_eq!(result["messages"][0]["flags"][0], "\\Flagged");
            assert_eq!(result["next_before_uid"], 12);
            assert_eq!(result["more"], true);
            drop(connection);
            handle.join().unwrap();
        }

        /// anchorbook: A-policy-triage#operation-filter
        #[test]
        fn flags_and_copies_use_uid_commands_with_scoped_folders() {
            let script = vec![
                (
                    "SELECT \"Projects\"".into(),
                    examine(7).replace("READ-ONLY", "READ-WRITE"),
                ),
                (
                    "UID STORE 12 +FLAGS.SILENT (\\Seen \\Flagged)".into(),
                    "{tag} OK flags updated\r\n".into(),
                ),
                (
                    "SELECT \"Projects\"".into(),
                    examine(7).replace("READ-ONLY", "READ-WRITE"),
                ),
                (
                    "UID COPY 12 Projects/Done".into(),
                    "{tag} OK copied\r\n".into(),
                ),
            ];
            let (mut connection, handle) = fixture(script);
            let result = connection
                .execute(
                    &account(),
                    MailRequest::Flags(FlagArgs {
                        configuration: "work".into(),
                        folder: ".".into(),
                        uids: vec![12],
                        uid_validity: 7,
                        action: FlagAction::Add,
                        flags: vec![TriageFlag::Seen, TriageFlag::Flagged],
                    }),
                )
                .unwrap();
            assert_eq!(result["updated_uids"][0], 12);
            let result = connection
                .execute(
                    &account(),
                    MailRequest::Copy(TransferArgs {
                        configuration: "work".into(),
                        folder: ".".into(),
                        destination: "Done".into(),
                        uids: vec![12],
                        uid_validity: 7,
                    }),
                )
                .unwrap();
            assert_eq!(result["moved"], false);
            drop(connection);
            handle.join().unwrap();
        }

        /// anchorbook: A-folder-confined#wire-boundaries
        #[test]
        fn unicode_scoped_folder_is_encoded_at_selection_boundary() {
            let (mut connection, handle) = fixture(vec![(
                "EXAMINE \"Projects/Entw&APw-rfe\"".into(),
                examine(7),
            )]);
            let result = connection
                .execute(
                    &account(),
                    MailRequest::Status(FolderArgs {
                        configuration: "work".into(),
                        folder: "Entwürfe".into(),
                    }),
                )
                .unwrap();
            assert_eq!(result["folder"], "Entwürfe");
            assert_eq!(result["uid_validity"], 7);
            drop(connection);
            handle.join().unwrap();
        }

        /// anchorbook: A-message-stable#stale-identifiers
        #[test]
        fn stale_uid_validity_stops_before_fetch() {
            let (mut connection, handle) =
                fixture(vec![("EXAMINE \"Projects\"".into(), examine(8))]);
            assert!(
                connection
                    .execute(
                        &account(),
                        MailRequest::Read(ReadArgs {
                            configuration: "work".into(),
                            folder: ".".into(),
                            uid: 12,
                            uid_validity: 7,
                        })
                    )
                    .is_err()
            );
            drop(connection);
            handle.join().unwrap();
        }

        /// anchorbook: A-message-stable#wire-safety
        #[test]
        fn move_uses_uid_move_without_close_or_expunge() {
            let script = vec![
                (
                    "SELECT \"Projects\"".into(),
                    examine(7).replace("READ-ONLY", "READ-WRITE"),
                ),
                (
                    "CAPABILITY".into(),
                    "* CAPABILITY IMAP4rev1 MOVE UIDPLUS\r\n{tag} OK capabilities\r\n".into(),
                ),
                (
                    "UID MOVE 12 \"Projects/Done\"".into(),
                    "{tag} OK moved\r\n".into(),
                ),
            ];
            let (mut connection, handle) = fixture(script);
            let result = connection
                .execute(
                    &account(),
                    MailRequest::Move(TransferArgs {
                        configuration: "work".into(),
                        folder: ".".into(),
                        destination: "Done".into(),
                        uids: vec![12],
                        uid_validity: 7,
                    }),
                )
                .unwrap();
            assert_eq!(result["moved"], true);
            assert!(result["destination_uids"].is_null());
            drop(connection);
            handle.join().unwrap();
        }

        /// anchorbook: A-message-stable#wire-safety
        #[test]
        fn unsupported_move_has_no_deletion_fallback() {
            let script = vec![
                (
                    "SELECT \"Projects\"".into(),
                    examine(7).replace("READ-ONLY", "READ-WRITE"),
                ),
                (
                    "CAPABILITY".into(),
                    "* CAPABILITY IMAP4rev1 UIDPLUS\r\n{tag} OK capabilities\r\n".into(),
                ),
            ];
            let (mut connection, handle) = fixture(script);
            assert!(
                connection
                    .execute(
                        &account(),
                        MailRequest::Move(TransferArgs {
                            configuration: "work".into(),
                            folder: ".".into(),
                            destination: "Done".into(),
                            uids: vec![12],
                            uid_validity: 7,
                        })
                    )
                    .is_err()
            );
            drop(connection);
            handle.join().unwrap();
        }

        /// anchorbook: A-policy-triage#draft-only
        #[test]
        fn draft_append_uses_draft_flag_and_scoped_destination() {
            let (client, server) = UnixStream::pair().unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            server
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let handle = thread::spawn(move || {
                let mut writer = server.try_clone().unwrap();
                let mut reader = BufReader::new(server);
                writer.write_all(b"* OK ready\r\n").unwrap();
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let (tag, _) = line.trim_end().split_once(' ').unwrap();
                writer
                    .write_all(format!("{tag} OK logged in\r\n").as_bytes())
                    .unwrap();
                line.clear();
                reader.read_line(&mut line).unwrap();
                let (tag, command) = line.trim_end().split_once(' ').unwrap();
                let tag = tag.to_owned();
                assert!(
                    command.starts_with("APPEND \"Projects/Drafts\" (\\Draft \\Seen) {"),
                    "{command}"
                );
                let length: usize = command
                    .rsplit_once('{')
                    .unwrap()
                    .1
                    .trim_end_matches('}')
                    .parse()
                    .unwrap();
                writer.write_all(b"+ ready for literal\r\n").unwrap();
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let parsed = mailparse::parse_mail(&body).unwrap();
                assert_eq!(parsed.headers.get_first_value("Subject").unwrap(), "Draft");
                assert_eq!(parsed.get_body().unwrap(), "Hello");
                let mut crlf = [0; 2];
                reader.read_exact(&mut crlf).unwrap();
                assert_eq!(&crlf, b"\r\n");
                writer
                    .write_all(format!("{tag} OK appended\r\n").as_bytes())
                    .unwrap();
                line.clear();
                assert_eq!(
                    reader.read_line(&mut line).unwrap(),
                    0,
                    "Unexpected command: {line}"
                );
            });
            let mut client = imap::Client::new(client);
            client.read_greeting().unwrap();
            let session = client
                .login("fixture", "fixture")
                .map_err(|(e, _)| e)
                .unwrap();
            let mut connection = Connection {
                session,
                scope: FolderScope::new("Projects".into(), Some('/')).unwrap(),
            };
            let draft = serde_json::from_value(json!({
                "configuration": "work", "from": "me@example.com", "subject": "Draft", "body": "Hello"
            })).unwrap();
            assert_eq!(
                connection
                    .execute(&account(), MailRequest::Draft(draft))
                    .unwrap()["sent"],
                false
            );
            drop(connection);
            handle.join().unwrap();
        }
    }
}

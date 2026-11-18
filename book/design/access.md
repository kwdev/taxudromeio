# Access and operations

Each request selects one configuration, whose policy and root apply together.

> **`A-policy-triage`** (draft): **Triage permits organization and drafts without sending**
> Property: The default profile permits reading, searching, moving, copying, marking read, flagging, and draft creation. Read-only and optional explicit operation allowlists narrow access. No profile or tool may send mail or explicitly delete or expunge mail.
> Authority: `R-triage-default`; `D-imap-only`
> - Criterion `operation-filter` [test]: Default triage includes the defined read and mutation operations, read-only excludes mutations, and empty or narrowing allowlists cannot widen a profile.
> - Criterion `direct-call-enforcement` [test]: An actual MCP session advertises the union of permitted tools; both MCP and parsed CLI calls reject operations disabled for the selected configuration before backend or credential access, including direct calls.
> - Criterion `draft-only` [test]: Draft creation produces valid MIME and appends with Draft and Seen flags inside scope; injected headers are refused, and the wire fixture observes no SMTP or send operation.

> **`A-folder-confined`** (draft): **Every accessed mailbox belongs to the configured root**
> Property: Tool results expose only the configured root and its descendants, and every source and destination mailbox used for content access or mutation remains inside that tree using the server's hierarchy delimiter.
> Authority: `R-root-scope`
> - Criterion `root-boundaries` [test]: Prefix siblings, traversal, absolute relative paths, control characters, and wildcards are excluded with slash, dot, and absent delimiters.
> - Criterion `wire-boundaries` [test]: The IMAP adapter filters listed siblings and refuses out-of-scope move or draft destinations before issuing a mailbox mutation.
> - Criterion `mailbox-quoting` [test]: Root discovery treats names containing spaces, colons and Unicode as one LIST argument. Copy and draft destinations escape quote and backslash characters, while selection and moving receive names quoted once by the IMAP library.

> **`A-message-stable`** (draft): **Message identifiers cannot silently change meaning**
> Property: Message content access and UID mutations require the configured folder's current UIDVALIDITY and positive UIDs. Reads preserve Seen; moves use UID MOVE without expunging unrelated deleted mail.
> Authority: `D-scoped-uids`; `R-triage-default`
> - Criterion `stale-identifiers` [test]: A changed or absent UIDVALIDITY and zero, empty or oversized UID sets are refused.
> - Criterion `wire-safety` [test]: Protocol fixtures observe EXAMINE and BODY.PEEK for reads, UID MOVE for moves, and no CLOSE, EXPUNGE or fallback deletion.

> **`A-interface-shared`** (draft): **CLI and MCP enforce the same operation boundary**
> Property: Every mail CLI command creates the same typed request as its MCP tool and executes through one shared service and IMAP backend. Selected-configuration policy, root scope, UIDVALIDITY, credential handling and JSON results apply equally to both. CLI calls require an explicit configuration ID and have no credential arguments.
> Authority: `R-cli-operations`; `R-root-scope`; `R-triage-default`; `D-shared-mail-service`
> - Criterion `adapter-parity` [test]: All nine parsed CLI operations and their equivalent MCP calls deliver the same typed arguments and JSON results through a fake backend, including pagination, search filters, flags and drafts.
> - Criterion `policy-boundary` [test]: Missing configurations and disabled operations fail through both interfaces before fake backend access; an actual CLI subprocess refuses a read-only mutation without touching Keychain.
> - Criterion `clap-boundary` [test]: Required IDs, destinations, UIDs and UIDVALIDITY, valid page limits, supported flags and search booleans are enforced by Clap; shared validation rejects oversized aggregate UID/flag lists before backend access. Sending, password arguments and unsupported commands are refused.

> **`A-cli-agent-help`** (draft): **Help provides concise command reference and guide discovery**
> Property: Top-level help lists commands and points to skill; nested help lists operation arguments, required fields and defaults. Detailed mail workflows, examples and mutation safety guidance live in skill rather than repeated help appendices.
> Authority: `R-cli-agent-help`; `R-project-name`; `R-cli-setup-help`
> - Criterion `workflow-help` [test]: Actual nested help exits without settings or credentials, lists all mail commands and their required configuration fields, and retains useful auth, transport, profile, selection and operation argument reference information.
> - Criterion `renamed-interface` [test]: Cargo builds taxudromeio, help and MCP identify that name, and existing configuration settings load without a storage identity change.

> **`A-cli-skill`** (draft): **The executable provides a complete Markdown usage guide**
> Property: skill writes detailed Markdown to stdout without reading configuration, Keychain or mail. It explains that login normally happens in the user's CLI, how an agent can launch interactive setup with a downloaded profile without exposing credentials, valid mail operations, scope and UIDVALIDITY, multiple IDs and CLI/MCP behavior. Examples agree with Clap and preserve the same access boundaries.
> Authority: `R-cli-skill`; `R-cli-operations`; `R-keychain-login`; `R-multiple-configurations`; `D-shared-mail-service`
> - Criterion `standalone-guide` [test]: The real skill command succeeds with a nonexistent settings path, writes Markdown without stderr diagnostics, and contains the detailed login, command, configuration and efficiency guidance beyond compact help.
> - Criterion `guide-examples` [test]: Shell-quoted taxudromeio examples extracted from the printed guide parse through Clap, cover all nine mail operations, and retain quoted folder and message text without executing auth or touching real services.

Folder paths are relative to the root. `.` means the root itself. A root such as
`INBOX` does not include a sibling named `Sent` or `Drafts`. Choosing a parent
namespace permits its children; each root must exist. An absent delimiter
permits only the root mailbox. Both transfer endpoints and draft destinations
use these rules. Folder discovery may receive server-wide mailbox names but
filters them locally before exposing metadata.
Unicode folder names use modified UTF-7 only at the IMAP wire boundary.
Mailbox names are serialized as quoted IMAP arguments. The adapter accounts for
which library calls quote names themselves: LIST patterns and UID COPY need
complete quoted arguments, SELECT/EXAMINE and UID MOVE accept unquoted names,
and APPEND needs escaped content inside the library's surrounding quotes.

The triage profile includes list folders, folder status, list messages, search,
read, move, copy, set flags, and save draft. Read-only includes the first five.
`allowed_operations`, when present, intersects the profile, including an empty
list that denies all account operations. Configuration discovery remains available.
No sending, raw IMAP, folder deletion, Deleted flag, or expunge tool exists.

Read methods use EXAMINE and BODY.PEEK. Search accepts typed fields rather than
raw IMAP expressions. Listing pages contain at most 100 messages and 16 KiB of
headers each. A message read accepts at most 5 MiB and returns at most 40,000
characters of MIME text plus attachment metadata. It does not download
attachments into the filesystem. HTML is returned as text, without rendering.

UID mutations operate on 1–100 positive identifiers and require UIDVALIDITY.
Moves require server support for MOVE; unsupported moves fail before mutation.
The server drops connections without CLOSE or EXPUNGE. Destination UIDs must
be retrieved by listing the destination after copying or moving.
Missing UIDs may have disappeared concurrently; IMAP commands can succeed
without changing every requested UID. Results identify requested UIDs, not a
verified affected count.

Drafts are new IMAP APPENDs with Draft and Seen flags. The destination is an
explicit relative folder or the configuration's drafts_folder. Recipients and
From are bare email addresses, subject is MIME encoded, and body is UTF-8 text.
Repetition creates another draft. Draft creation does not replace an existing draft.

`taxudromeio mail` offers `folders`, `status`, `list`, `search`, `read`, `move`,
`copy`, `flags` and `draft`. Each command requires `--configuration ID`.
The same argument structs implement Clap parsing and MCP deserialization.
Both adapters enter the shared mail service for per-configuration authorization
and bounded execution. Request-wide page, UID and flag limits are checked before
credential access, including lists expanded from commas or repeated CLI options.
`configurations` prints the same nonsecret discovery
object as MCP, including permitted operation names. CLI success writes one JSON
value to stdout; errors go to stderr with a nonzero exit status.
Operation subcommand help lists argument defaults and required fields.
Search booleans accept a bare option for true or `--unread=false` /
`--flagged=false` for false. CLI flags and recipients can be repeated; UIDs
also accept comma-separated values. Draft bodies are passed with `--body`.

`taxudromeio skill` prints the embedded Markdown source in docs/skill.md.
It is available before setup and may be redirected into a guide file.
Both `--help` and `-h` point callers to it while retaining a compact command
overview. Auth, serve and mail help keep flags, defaults and brief operational
notes. Detailed setup, command examples and MCP JSON live in skill.
The guide treats CLI authentication as the normal login workflow. Agents can
launch auth with a user-selected downloaded profile in an interactive terminal,
use file paths rather than disclose embedded credentials, and hand secret input
to the user when needed. Configuration scope and policy remain local choices.

The MCP process retains loaded settings, per-configuration refresh locks and
worker limits across calls; standalone CLI commands create separate processes.
Both use a new IMAP connection for every operation. The guide distinguishes
process/discovery conveniences from unmeasured latency and token costs, and
does not imply IMAP connection pooling or claim a performance benchmark.

Four blocking workers limit concurrent connections. Socket connect and I/O use
30-second timeouts. DNS resolution and a sequence of commands have no total
deadline. A cancelled MCP request may already have changed IMAP state; callers
must inspect the mailbox before retrying a mutation.

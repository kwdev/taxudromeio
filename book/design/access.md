# Access and operations

Each request selects one configuration, whose policy and root apply together.

> **`A-policy-triage`** (draft): **Triage permits organization and drafts without sending**
> Property: The default profile permits reading, searching, moving, copying, marking read, flagging, and draft creation. Read-only and optional explicit operation allowlists narrow access. No profile or tool may send mail or explicitly delete or expunge mail.
> Authority: `R-triage-default`; `D-imap-only`
> - Criterion `operation-filter` [test]: Default triage includes the defined read and mutation operations, read-only excludes mutations, and empty or narrowing allowlists cannot widen a profile.
> - Criterion `direct-call-enforcement` [test]: An actual MCP session advertises the union of permitted tools and rejects operations disabled for the selected configuration before backend or credential access, including direct calls.
> - Criterion `draft-only` [test]: Draft creation produces valid MIME and appends with Draft and Seen flags inside scope; injected headers are refused, and the wire fixture observes no SMTP or send operation.

> **`A-folder-confined`** (draft): **Every accessed mailbox belongs to the configured root**
> Property: Tool results expose only the configured root and its descendants, and every source and destination mailbox used for content access or mutation remains inside that tree using the server's hierarchy delimiter.
> Authority: `R-root-scope`
> - Criterion `root-boundaries` [test]: Prefix siblings, traversal, absolute relative paths, control characters, and wildcards are excluded with slash, dot, and absent delimiters.
> - Criterion `wire-boundaries` [test]: The IMAP adapter filters listed siblings and refuses out-of-scope move or draft destinations before issuing a mailbox mutation.

> **`A-message-stable`** (draft): **Message identifiers cannot silently change meaning**
> Property: Message content access and UID mutations require the configured folder's current UIDVALIDITY and positive UIDs. Reads preserve Seen; moves use UID MOVE without expunging unrelated deleted mail.
> Authority: `D-scoped-uids`; `R-triage-default`
> - Criterion `stale-identifiers` [test]: A changed or absent UIDVALIDITY and zero, empty or oversized UID sets are refused.
> - Criterion `wire-safety` [test]: Protocol fixtures observe EXAMINE and BODY.PEEK for reads, UID MOVE for moves, and no CLOSE, EXPUNGE or fallback deletion.

Folder paths are relative to the root. `.` means the root itself. A root such as
`INBOX` does not include a sibling named `Sent` or `Drafts`. Choosing a parent
namespace permits its children; each root must exist. An absent delimiter
permits only the root mailbox. Both transfer endpoints and draft destinations
use these rules. Folder discovery may receive server-wide mailbox names but
filters them locally before exposing metadata.
Unicode folder names use modified UTF-7 only at the IMAP wire boundary.

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

Four blocking workers limit concurrent connections. Socket connect and I/O use
30-second timeouts. DNS resolution and a sequence of commands have no total
deadline. A cancelled MCP request may already have changed IMAP state; callers
must inspect the mailbox before retrying a mutation.

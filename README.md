# imsg

CLI over the local iMessage database (`~/Library/Messages/chat.db`). Built to give Claude (and humans) a clean, scriptable interface to messages: list chats, read conversations by contact name, search text, locate attachments — and send messages through Messages.app. Reading is strictly read-only against the database; sending never touches it (it goes through AppleScript, and Messages.app owns the write path).

## Why

The iMessage SQLite schema is hostile to direct querying:

- Since macOS Ventura, most message bodies are **not** in `message.text` — they live in `message.attributedBody` as a serialized `NSAttributedString` (Apple typedstream binary format).
- Timestamps are nanoseconds since 2001-01-01 (Apple epoch).
- Senders are opaque `handle` rows (raw phone numbers / emails), not contact names.
- Tapbacks, edits, and replies are separate rows linked by GUID.

`imsg` handles all of that behind a `kdb`-style noun-verb CLI with human transcript output by default and `--json` everywhere.

## Install

```sh
cargo install --path crates/cli
```

Requires **Full Disk Access** for your terminal (System Settings → Privacy & Security → Full Disk Access) so it can read `chat.db`. Run `imsg doctor` to verify.

## Usage

```text
imsg
  chats list        [--limit N] [--json]            # chats by last activity
  chats show <id>   [--json]                        # participants + metadata

  messages list     (--contact <name|phone|email> | --chat <id>)
                    [--since <date>] [--until <date>] [--limit N]
                    [--from-me | --from-them] [--attachments-only]
                    [--unread] [--json]          # --unread sweeps all chats
                                                 # when no selector is given
  messages search   <query> [--contact ...] [--chat <id>]
                    [--since <date>] [--until <date>] [--limit N] [--json]

  contacts list     [--json]                        # handles + resolved names + counts
  contacts resolve  <name|phone|email> [--json]     # name → handles/chats mapping

  attachments list  (--contact ... | --chat <id>)
                    [--since <date>] [--limit N] [--json]

  send              (--to <name|phone|email> | --chat <id>) <text>
                    [--yes] [--dry-run] [--json]    # via Messages.app

  doctor                                            # DB access, schema, decode rate
```

A blocklist at `~/.config/imsg/blocklist` hides chosen conversations from every command — see [Blocklist](#blocklist).

### Examples

```sh
imsg chats list --limit 20                       # includes an UNREAD column
imsg messages list --unread --since 2w           # anything I haven't read?
imsg messages list --contact mom --since 2026-06-01
imsg messages search "dinner" --contact jake --limit 20
imsg attachments list --chat 42 --json
```

Default output is a compact transcript, one message per line, tapbacks folded into their target message:

```text
[2026.06.28 14:32] Mom: are you coming sunday?
[2026.06.28 14:35] me: yeah, around noon
    ❤️ Mom
[2026.06.29 09:02] Mom: <attachment: IMG_2041.heic (image/heic, 2.1 MB)>
```

### Contact matching

`--contact` accepts a name fragment (`mom`, `jake`), a phone number in any format, or an email. Ambiguous queries fail with exit code 2 and list the candidates — the tool never silently picks one.

### Sending

`imsg send` delivers through Messages.app via AppleScript — never by writing the database. It requires **Automation** permission for your terminal (System Settings → Privacy & Security → Automation → Messages) on top of Full Disk Access.

```sh
imsg send --to ross "running 10 late"        # confirms y/N before sending
imsg send --chat 194 --yes "on my way"       # group chats by explicit id only
imsg send --to "+14155551234" --dry-run "hi" # resolve + preview, don't send
```

Safety properties:

- **Confirm by default** — interactive `y/N` prompt; non-interactive use refuses without `--yes`, so a script can never send by accident.
- **Verified sends** — AppleScript exit codes are meaningless (mis-targeted sends no-op silently), so every send is confirmed by polling `chat.db` for the new outgoing row and reporting its `is_sent`/`error` state. No row within 10s is a hard error.
- **No implicit group sends** — `--to` only ever targets a 1:1 chat (or creates one); groups require an explicit `--chat` id.
- Message text is passed to `osascript` as an argument, never interpolated into script source.

### Unread

`--unread` filters to inbound messages you haven't read; without `--contact`/`--chat` it sweeps every chat, which makes `imsg messages list --unread --since 2w` a one-shot triage. `chats list` carries the same count per chat. Caveat: messages predating read receipts can sit at `is_read = 0` forever, so pair with `--since` and treat counts as an upper bound. Read-only like everything else — imsg never marks anything read.

### Blocklist

`~/.config/imsg/blocklist` lists conversations imsg refuses to touch — one entry per line: a contact name, phone, email, or `chat:<id>` (`#` for comments). Enforcement is deny-by-default in core, so every surface respects it: blocked chats vanish from `chats list`, message/search/unread/attachment queries exclude them at the SQL layer, any group containing a blocked contact is hidden entirely, and `send` refuses (exit 2). `imsg doctor` reports how much is hidden without saying what.

Honest scope: this governs the imsg tool, not the database — anything with Full Disk Access can still read `chat.db` directly. It's a guardrail for agents driving imsg, not encryption.

### Dates

`--since`/`--until` accept `YYYY-MM-DD`, `YYYY-MM-DDTHH:MM`, or relative forms `7d`, `24h`, `2w`.

### JSON output

`--json` emits a stable array schema for machine consumption:

```json
{
  "id": 4821,
  "chat_id": 42,
  "date": "2026-06-28T14:32:11-07:00",
  "sender": { "handle": "+14155551234", "name": "Mom", "is_me": false },
  "text": "are you coming sunday?",
  "service": "iMessage",
  "reply_to": null,
  "edited": false,
  "tapbacks": [{ "kind": "love", "by": "me" }],
  "attachments": [{ "filename": "IMG_2041.heic", "mime": "image/heic", "size": 2201394, "path": "~/Library/Messages/Attachments/..." }]
}
```

## Guarantees

- **Read-only database**: `chat.db` and the AddressBook are opened with SQLite read-only flags; there is no code path that writes to either. Sending goes through Messages.app, which owns the write path.
- **Local-only**: no network access of its own; nothing leaves the machine except messages you explicitly send.
- Contact names resolve from the local AddressBook (`~/Library/Application Support/AddressBook`). If unavailable, raw handles are shown instead — everything still works.

## Architecture

Cargo workspace, two crates:

| Crate | Path | Role |
|---|---|---|
| `imsg-core` | `crates/core` | DB access, typedstream decoding (via [`imessage-database`](https://crates.io/crates/imessage-database)), contact resolution, domain types |
| `imsg` | `crates/cli` | clap CLI, transcript/JSON rendering, send (osascript wrapper + confirm/verify) |

See `docs/impl-plan.md` for the full design.

## License

GPL-3.0-or-later (inherited from the `imessage-database` dependency).

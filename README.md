# imsg

Read-only CLI over the local iMessage database (`~/Library/Messages/chat.db`). Built to give Claude (and humans) a clean, scriptable interface to message history: list chats, read conversations by contact name, search text, locate attachments. It never writes to the database and it cannot send messages.

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
                    [--from-me | --from-them] [--attachments-only] [--json]
  messages search   <query> [--contact ...] [--chat <id>]
                    [--since <date>] [--until <date>] [--limit N] [--json]

  contacts list     [--json]                        # handles + resolved names + counts
  contacts resolve  <name|phone|email> [--json]     # name → handles/chats mapping

  attachments list  (--contact ... | --chat <id>)
                    [--since <date>] [--limit N] [--json]

  doctor                                            # DB access, schema, decode rate
```

### Examples

```sh
imsg chats list --limit 20
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

- **Read-only**: the database is opened with SQLite read-only flags; there is no code path that writes to `chat.db` or the AddressBook.
- **Local-only**: no network access; nothing leaves the machine.
- Contact names resolve from the local AddressBook (`~/Library/Application Support/AddressBook`). If unavailable, raw handles are shown instead — everything still works.

## Architecture

Cargo workspace, two crates:

| Crate | Path | Role |
|---|---|---|
| `imsg-core` | `crates/core` | DB access, typedstream decoding (via [`imessage-database`](https://crates.io/crates/imessage-database)), contact resolution, domain types |
| `imsg` | `crates/cli` | clap CLI, transcript/JSON rendering |

See `docs/impl-plan.md` for the full design.

## License

GPL-3.0-or-later (inherited from the `imessage-database` dependency).

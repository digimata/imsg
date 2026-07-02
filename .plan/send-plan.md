# imsg send — scoping & design

status: scoped (spike verified live 2026.07.01)

## Feasibility: confirmed

Sending works via `osascript` → Messages.app. Verified live on this machine
(macOS Darwin 25.5, Messages with 6 accounts):

| Route | Result |
|---|---|
| `send "…" to participant "<handle>"` (bare) | **Silent no-op.** Exit 0, no row ever appears in chat.db. Never use. |
| `send "…" to participant "<handle>" of account id "<id>"` | ✅ Works. Creates the chat if none exists; row lands with `is_sent=1, error=0`. |
| `send "…" to chat id "<guid>"` | ✅ Works (verified on a 1:1 chat guid). Group chats use the same form (`iMessage;+;chat…` guids) — mechanism validated, group send not live-tested. |

Other findings from the spike:

- **Exit code 0 is meaningless.** The bare-participant form no-ops silently. The
  only trustworthy confirmation is a new `is_from_me=1` row in chat.db for the
  target chat with `is_sent=1, error=0`. Send must be followed by a verify poll.
- **Account enumeration is flaky.** `get id of every account` works, but
  property reads (`service type`, `description`) throw `-10000` on some
  accounts. Enumerate ids, then probe each inside a `try` block; pick the
  enabled account with `service type = iMessage`.
- **Automation permission** (System Settings → Privacy & Security → Automation
  → terminal → Messages) is required; osascript error `-1743` when denied.
  Already granted on this machine.
- Messages.app does not need to be frontmost, but must be signed in.

## Design

```
imsg send (--to <contact|handle> | --chat <id>) <text>
          [--yes] [--dry-run] [--json]
```

### Flow

1. **Resolve recipient.**
   - `--to`: reuse `ContactBook::resolve` + `chats::resolve_selector` (same
     ambiguity policy: >1 match → exit 2 with candidates). If an existing chat
     is found, target it by guid (uniform for 1:1 and groups). If the contact
     has no chat yet, fall back to account-qualified `participant` send with
     the contact's best handle (creates the chat).
   - `--chat <rowid>`: look up guid + display name; errors with `NoChat`.
2. **Confirm.** Print resolved recipient (name, handle/chat, service) and the
   text; interactive `y/N` prompt. `--yes` skips; `--dry-run` stops here.
3. **Send.** Spawn `/usr/bin/osascript` passing the script with `on run argv`
   and the text + target as **arguments** (never interpolated into the script
   source — no escaping bugs, no injection).
4. **Verify.** Record `MAX(message.ROWID)` before sending; poll chat.db
   (~500ms interval, 10s timeout) for a new `is_from_me=1` row joined to the
   target chat. Report `is_sent`/`error`. Timeout → hard error telling the
   user to check Messages.app (this is the silent-no-op guard).
5. **Output.** Human: one confirmation line with the new message rowid and
   timestamp. `--json`: `{rowid, chat_id, date, recipient, text, is_sent}`.

### Layering

- Send lives in the CLI crate (`crates/cli/src/cmd/send.rs`) plus a small
  `osascript` wrapper module. Core stays read-only by construction — the only
  additions to core are read helpers (`chats::guid_for`, newest-rowid /
  verify-poll queries), all `SELECT`s.
- chat.db is never written. Delivery goes through Messages.app, which owns
  the write path.

### Errors (new variants)

- `NoIMessageAccount` — no enabled iMessage account found
- `AutomationDenied` — osascript `-1743`, with the System Settings hint
- `SendUnverified` — verify poll timed out (exit 1, loud)

### Out of scope v1 (backlog)

- Attachments (`send POSIX file … to …` works in AppleScript — add `--attach`)
- SMS/RCS fallback selection
- Group-chat creation (targeting existing groups by guid should work; creating
  new ones via AppleScript is flaky)

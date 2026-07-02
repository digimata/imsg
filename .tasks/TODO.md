# imsg — tasks

## Backlog

- [ ] **Unread awareness** — surface unread inbound messages so "anything I haven't answered?"
  is one command. chat.db has `message.is_read`/`date_read` (verified live: `is_read = 0 AND
  is_from_me = 0` returns sensible rows). Two pieces: (1) `--unread` filter on `messages list`
  (compose with existing `--contact`/`--chat`/window flags — add a predicate to `MessageQuery`
  in core `messages.rs`); (2) an UNREAD count column in `chats list` (extend the summary query
  in core `chats.rs`). Gotcha: old messages predating read receipts may sit permanently at
  `is_read = 0`, so pair with `--since` in examples and don't treat the raw count as gospel.
  Read-only throughout — never write `is_read`.

- [ ] **Send attachments** — `imsg send --attach <path>`. AppleScript supports
  `send POSIX file "…" to …`; reuse the existing confirm/verify flow. Verify the attachment row
  appears alongside the message. Deferred from send v1.

## Done

- [x] **Send messages** (2026.07.01) — `imsg send (--to <contact|handle> | --chat <id>) <text>`
  with confirm-by-default, `--yes`, `--dry-run`, `--json`. Design in `.plan/send-plan.md`.
  Implemented per plan: chat-guid targeting for existing chats, account-qualified participant
  fallback for first contact, chat.db verify poll (osascript exit codes are meaningless), groups
  only by explicit `--chat`. Live-verified both directions with a real conversation.

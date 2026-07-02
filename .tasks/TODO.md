# imsg — tasks

## Backlog

- [ ] **Send attachments** — `imsg send --attach <path>`. AppleScript supports
  `send POSIX file "…" to …`; reuse the existing confirm/verify flow. Verify the attachment row
  appears alongside the message. Deferred from send v1.

## Done

- [x] **Send messages** (2026.07.01) — `imsg send (--to <contact|handle> | --chat <id>) <text>`
  with confirm-by-default, `--yes`, `--dry-run`, `--json`. Design in `.plan/send-plan.md`.
  Implemented per plan: chat-guid targeting for existing chats, account-qualified participant
  fallback for first contact, chat.db verify poll (osascript exit codes are meaningless), groups
  only by explicit `--chat`. Live-verified both directions with a real conversation.

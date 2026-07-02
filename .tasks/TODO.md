# imsg — tasks

## Backlog

- [ ] **Send messages** — add `imsg send (--to <contact|handle> | --chat <id>) <text>` with
  confirm-by-default (`--yes` to skip), `--dry-run`, `--json`. **Scoped and spike-verified
  2026.07.01** — full design in `.plan/send-plan.md`. Route: osascript → Messages.app; target
  existing chats by guid, fall back to account-qualified `participant` for first contact. Key
  gotchas already proven live: bare `participant` sends silently no-op (exit 0, nothing delivered),
  so every send must be verified by polling chat.db for the new row; account property reads throw
  -10000 and need per-id try blocks. Send lives in the CLI crate; core gains only read-only
  helpers (guid lookup, verify poll). Ready to implement.

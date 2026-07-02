# imsg — tasks

## Backlog

- [ ] **Send messages** — add an `imsg send` command (e.g. `imsg send --to <contact|chat> <text>`).
  chat.db is not writable (and writing it wouldn't deliver anything); sending has to go through
  AppleScript (`osascript` → Messages.app) or the Shortcuts CLI. Scope: 1:1 sends by contact/handle
  first (AppleScript group-chat targeting is flaky), reuse the core contact resolver for `--to`,
  require an explicit `--yes` or print a confirm prompt by default since this is an outward-facing
  action. Keep it in the CLI crate; core stays read-only by construction.

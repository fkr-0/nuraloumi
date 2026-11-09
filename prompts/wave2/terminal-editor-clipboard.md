@projmgrauth
Set repo: nuraloumi

# NuraLoumi Wave 2 — Terminal, Editor, and Clipboard Surfaces (0.0.7)

Lane: terminal-editor-clipboard
Priority: 86

## Mission

Create three new surface binaries that leverage the SL101 slider keyboard:
`nuraloumi-terminal` (quick terminal launch), `nuraloumi-editor` (editor
launch), and `nuraloumi-clipboard` (clipboard history). These are
thin launcher/display surfaces, not terminal emulators.

Read:
- AGENTS.md, DESIGN.md, ROADMAP.md (0.0.7 section)
- docs/ARCHITECTURE.md, docs/CONTRACTS.md
- crates/nuraloumi-providers/src/command.rs (CommandRunner, CommandSpec, CommandLimits)
- crates/nuraloumi-shell/src/bin/run.rs (existing app-launch pattern)
- crates/nuraloumi-shell/src/bin/windows.rs (dedicated binary pattern)

## Exclusive write scope

- crates/nuraloumi-shell/src/bin/terminal.rs (new)
- crates/nuraloumi-shell/src/bin/editor.rs (new)
- crates/nuraloumi-shell/src/bin/clipboard.rs (new)
- crates/nuraloumi-shell/Cargo.toml ([[bin]] entries only)

Do not edit providers, renderer, Wayland backend, or core model.

## nuraloumi-terminal binary

Quick terminal launcher. Opens the configured terminal emulator with
NuraLoumi-aware defaults, then exits (fire-and-forget).

### Behavior
1. Read terminal command from config `[providers] terminal` (default: "foot").
2. Spawn the terminal as a detached child process.
3. The NuraLoumi surface itself closes after launch (one-shot).
4. Optional `--command CMD` flag: pass CMD to the terminal
   (e.g. `nuraloumi-terminal --command "htop"` runs `foot -e htop`).

### Terminal-specific launch args
- **foot**: `foot -e CMD` for command mode. Font size from config or
  `--font-size N` flag (default: 11, good for SL101 1280x800).
- **Other terminals**: pass command after `--` or `-e` depending on
  terminal. Config `[surface.terminal] launch_args = ["-e"]` for override.

### Interaction
- This is a one-shot launcher, not a persistent surface.
- If the terminal binary is not found, show a NuraLoumi error surface
  (single-row menu: "Terminal 'foot' not found") for 3 seconds, then exit.

## nuraloumi-editor binary

Editor launcher. Opens the configured editor, optionally inside a terminal.

### Behavior
1. Read editor from config `[providers] editor` (default: value of `$EDITOR`,
   fallback "nvim").
2. Determine launch mode:
   - **Terminal editors** (nvim, vim, nano, emacs -nw, helix): wrap in
     terminal command: `foot -e nvim`.
   - **Graphical editors** (emacs without -nw, code, etc.): launch directly.
   - Config override: `[surface.editor] graphical = true/false`.
3. Optional `--file PATH` flag: open a specific file.
4. One-shot launcher (same as terminal binary).

### Editor detection
Default heuristic for terminal vs graphical:
- Known terminal: nvim, vim, vi, nano, helix, micro, emacs -nw, emacs -Q -nw
- Known graphical: emacs (without -nw), gvim, code, kate
- Config override wins.

## nuraloumi-clipboard binary

Clipboard history surface reading from wl-copy/wl-paste (wl-clipboard).

### Layout
- **Header**: "Clipboard History" with entry count.
- **Entry list**: most recent clipboard entries, each showing truncated
  text preview (first 80 chars, single line).
- **Empty state**: "Clipboard empty" when no history.

### Data source
Use `wl-paste --list-types` to check if clipboard has content, and
`wl-paste` to read current content. For history, use `cliphist list`
if available (cliphist is a common Wayland clipboard manager), otherwise
show only the current clipboard entry.

Implementation via the existing `CommandRunner`:
```
CommandSpec::new("cliphist").arg("list") -> parse lines as history
CommandSpec::new("wl-paste") -> current clipboard fallback
```

### Interaction
- Arrow keys to navigate history entries.
- Enter on an entry: copy it back to clipboard via
  `echo "CONTENT" | wl-copy` (pipe through CommandRunner).
- `d` or Delete: remove entry from cliphist (`cliphist delete`).
- Search/filter to find entries.
- `q` or Escape to close.

### Graceful degradation
- No cliphist: show only current clipboard content as single entry.
- No wl-paste: show error "wl-clipboard not installed".
- Empty clipboard: show empty state row.

## Shared patterns
- CLI args: `--config PATH`, `--fixture`.
- Terminal and editor binaries are one-shot (exit after launch).
- Clipboard is a persistent surface (layer-shell popup).
- Follow existing binary structure and render pipeline.

## Constraints
- No new Rust dependencies. External tools (foot, wl-paste, cliphist) are
  optional runtime dependencies.
- CommandRunner bounds: 2-second timeout, 64 KiB output limit for clipboard.
- Clipboard text entries only (no image preview in v1).
- ARMv7/musl cross-build must pass.
- One-shot binaries must not leave zombie child processes.

## Acceptance
1. `nuraloumi-terminal` launches foot (or reports error if not found).
2. `nuraloumi-editor` launches nvim inside foot.
3. `nuraloumi-clipboard --fixture` shows demo clipboard entries.
4. Paste-on-select works in clipboard surface.
5. All three binaries in Cargo.toml [[bin]] entries.
6. `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass.

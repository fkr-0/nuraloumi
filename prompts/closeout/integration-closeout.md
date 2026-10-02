@projmgrauth
Set repo: nuraloumi

# NuraLoumi Integration Closeout

Canonical OCP task: NURALOUMI-INTEGRATION-CLOSEOUT-20261002
Priority: 100
Lane: integration-closeout

Mission: finish the currently dirty integration slices as separate coherent commits, never as one giant commit. Preserve active claims and do not touch a path while another packet owns it.

Predecessor/current work to reconcile:
- provider dependency cleanup packet
- Cairo SL101 density/layout packet
- foreign-toplevel shell E2E packet
- awaiting-review toplevel shell adapter
- awaiting-review screenshot UI fixups
- awaiting-review Tegra20 libm-free font sizing

Required sequence:
1. Inspect workflow packet/review state first.
2. Do not duplicate active implementation. Wait for or review existing results; only take ownership after claims clear.
3. Land provider dependency cleanup as its own commit.
4. Land Cairo density/layout/text/fixture slice as its own commit.
5. Land shell/window-control E2E as its own commit.
6. Update docs/menu-integration/README.md so Wi-Fi full scan projection and writable-backlight consistency are described as implemented, not future wiring.
7. Re-run cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace; cargo build --workspace --release.
8. Review git status/diff and leave no accidental generated artifacts.
9. Request independent review of the integrated closeout.

Acceptance: separate clean commits, full gates green, docs current, no active claim violated, working nuraloumi-menu/panel/probe/release binaries preserved.

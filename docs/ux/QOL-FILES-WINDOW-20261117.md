# NuraLoumi file manager window — lane 07

## Source-grounded audit

- `crates/nuraloumi-shell/src/bin/files.rs` implements a menu-oriented, rooted file browser (4 rows per page), not a desktop toplevel.
- `crates/nuraloumi-shell/src/file_manager.rs` implements pinned-root safety and bounded 512-entry batches, but not a sortable Nautilus-style details table.
- `crates/nuraloumi-wayland/**` currently exposes layer-shell; no `xdg_wm_base` / `xdg_toplevel` support was found. A true Sway window must not masquerade as a layer-shell popup.
- `crates/nuraloumi-shell/Cargo.toml` explicitly sets `autobins = false`, and is concurrently modified. Adding a source file does **not** register a binary.

## Implemented in owned files

- `src/file_manager_window.rs`: renderer-neutral details snapshot, asc/desc sort for name/type/size/mtime, stable selection by filename, bounded location editing/autocomplete, path breadcrumbs/up, local sidebar places and typed directory actions.
- `src/bin/file_manager_window.rs`: executable entrypoint stub with a bounded `--probe`; graphical invocation refuses to claim a working window until the backend seam lands.
- `tests/file_manager_window_model.rs`: sorting/selection, symlink and traversal rejection, large directory and completion bounds, root navigation/sidebar mapping.

Important limitations: only 512 entries are retained, with a visible truncation indicator; **pagination and globally correct sorting over enormous directories are not yet implemented**. The existing rooted browser has its own cursor logic but sorts by name. Directory scans/autocomplete are synchronous in this model and **must** be invoked on a worker rather than the rendering thread. Open actions are intents only; the integrator must verify file identity again and use the existing `nuraloumi_providers::file_open` backend. Favorites persistence, mounted-volume discovery, viewer/player handoff, and a true rendered table are not yet connected. This is **not** a completed GUI.

## Narrow ownership handoff for lane 02 (Wayland) and lane 15 (integration)

1. Lane 02 adds a reusable `xdg_wm_base` / `xdg_surface` / `xdg_toplevel` role in `nuraloumi-wayland`, with title/app_id, configure/ack, resize, keyboard/pointer/touch events, shm attach/commit, frame callbacks, decoration negotiation and clean close. Do not add EGL/GLES requirements. Verify under nested Sway where possible.
2. Lane 15 registers a new explicit `[[bin]]` in `crates/nuraloumi-shell/Cargo.toml`:

       [[bin]]
       name = "nuraloumi-file-manager"
       path = "src/bin/file_manager_window.rs"

   **Do not register the current stub for release.** Replace its fail-closed path with actual xdg-shell run loop first; verify a decorated, movable window in Sway. Coordinate packaging/desktop entry with lane 14.
3. Lane 07 / integrator uses `Details` snapshots off-thread to populate the table and render hit regions with >=48 logical-pixel touch targets; wire column clicks and scrolling, breadcrumb text-mode, sidebar actions and selected-item open-through-provider. Persist favorites and window placement only when compositor permits.
4. Preserve `bin/files.rs` behavior. No destructive file operations, no shell string execution, no recursive deletion, no renderer-side commands.

## Host qualification

    rustc --edition=2021 --test crates/nuraloumi-shell/tests/file_manager_window_model.rs -o /tmp/nuraloumi-file-window-model-tests
    /tmp/nuraloumi-file-window-model-tests

Host source tests are not Wayland or target-device evidence. ARMv7 VFPv3-D16/no-NEON qualification and physical Sway interaction remain open.

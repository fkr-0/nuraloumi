# Nura Browser — ownership and technical integration

## Canonical owner: NuraLoumi

The NuraBrowser application now lives in the NuraLoumi repository alongside
the existing editor, files, viewer and music applications. Its graphics use
NuraLoumi's Rust/Cairo/Wayland stack, **never GTK** or page-injected HTML.
The historical source in the SL101 Linux repository is preserved as a
reference until independent release qualification and an explicitly approved
handoff. No existing device binary is replaced by this migration.

    nuraloumi-browser (Rust)
        browser/mod.rs        typed model, address editing, tabs, bookmarks
        browser/transport.rs  authenticated, bounded same-UID AF_UNIX IPC
        nuraloumi-core       stable actions and selection
        nuraloumi-render-cairo  fixed 48px tab strip and menus
        nuraloumi-wayland     wl_shm layer-shell control surface
                 |
           private 0600 socket
                 |
    components/nura-browser-engine (internal C executable)
        control     bounded JSONL and same-UID peer authentication
        navigation  up to six distinct WebKitWebView instances
        bookmarks   durable private atomic state
        engine      WPEPlatform 2.54+ toplevel / WebProcess

There are intentionally two processes within one application: NuraLoumi's
control UI must never require EGL/WPE just to display a menu, and WebKit must
not become a required runtime dependency of the panel or file manager.
The native engine remains a separate executable installed to the internal
libexec/nuraloumi/browser directory. Meson does not install the historical
public ,browse launcher. Only NuraLoumi owns the public UI binary.

## Behavior and security

- NuraLoumi exposes an edit-capable address row, Back/Forward/Reload/Stop,
  a fixed horizontal tab strip, tab create/select/close and a paginated
  bookmarks list. The Rust menu supports offline fixtures without WebKit.
- Each native tab is a separate WebKitWebView object with its own navigation
  history. WPEPlatform attachment to a shared toplevel is explicitly checked
  and fails rather than pretending tab rendering succeeded.
- The bookmarks database holds at most 64 HTTP(S) URLs; navigation and
  bookmark storage use the same URL validator. Files reside under the
  private XDG state directory (0700, no symlink components) with a regular
  file mode 0600, exclusive private temporary inode, atomic rename and fsync.
- The in-memory Recent list is distinct from durable bookmarks. Long URLs
  are omitted from the optional Recent overview rather than truncating a
  navigated address into a different target.
- Only the same effective UID may use the Unix socket. Both ends check
  kernel peer credentials; requests, responses, URL lengths and queues are
  bounded. No shell command, TCP server, DOM injection or arbitrary JavaScript
  is involved in the browser control channel.
- The control surface never automatically starts an arbitrary WPE process
  or bypasses the protected ARMHF runtime root/sandbox gates. An unavailable
  native engine is a visible error, not an implicit stub success.

## Local build and verification

The Arch host has genuine WPE-WebKit and WPEPlatform development headers
2.54.2 installed; this is only a host development toolchain. Source builds
remain in a temporary Meson directory and an isolated Cargo target.

    meson setup /tmp/nuraloumi-browser-engine-native components/nura-browser-engine
    meson compile -C /tmp/nuraloumi-browser-engine-native
    meson test -C /tmp/nuraloumi-browser-engine-native --print-errorlogs

    CARGO_TARGET_DIR=target/nura-browser-migration-20261113 cargo test --workspace
    CARGO_TARGET_DIR=target/nura-browser-migration-20261113 cargo clippy --workspace --all-targets -- -D warnings
    cargo run -p nuraloumi-shell --bin nuraloumi-browser -- --fixture --headless

## Unresolved release gates — do not deploy

1. Prove native WPE WebProcess sandbox startup under the real private glibc
   environment. A genuine x86_64 headless IPC/tab probe succeeded but emitted
   a WebKit bus-proxy/helper sandbox error; not a passing renderer E2E.
2. Verify **ARMv7 VFPv3-D16 without NEON** application and complete shared
   library dependency closure. Native host Meson success does not satisfy it.
3. Validate a live Wayland compositor: WPE page surface, NuraLoumi control
   layer positioning, keyboard/touch focus, buffer pixels and tab handoff.
4. Implement and qualify supervised engine launch, process lifecycle and
   crash recovery with NuraLoumi. Existing Cog diagnostics remain intact.
5. Migrate/validate the historical SL101 release scripts and only then
   retire previous packaging after independently reviewed acceptance.

No GitHub, push, external CI, device mutation, or compositor restart was
authorized or performed as part of this migration.

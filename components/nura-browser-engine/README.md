# Internal NuraLoumi WPE browser engine

**NuraLoumi owns the browser application.** This directory is the internal C
WebKit 2.54+/WPEPlatform component. The graphical browser is
nuraloumi-browser, implemented in Rust using NuraLoumi Core, Cairo and
Wayland wl_shm. **There is no GTK or page-injected browser chrome.**

The engine is deliberately a separate process, not a separate project. It
retains WebKit lifecycle, tab objects, session resume and private same-UID
AF_UNIX control. It does not install a second public browser command. The
old SL101 Linux nura-browser source remains untouched as a migration
reference until independently reviewed replacement acceptance.

## Source boundaries

- src/engine: WebKitWebView / WPEPlatform process, single GLib event thread,
  sandboxed WebProcess; no GUI toolkit
- src/navigation: six actual WebKit tab instances, validated URLs, in-memory
  recent visits and saved resume state
- src/control: bounded JSONL AF_UNIX control using kernel peer credentials
- src/bookmarks: private atomic durable bookmark JSON, max 64 entries
- include/nurabrowser/contract.h: small native engine/control ABI
- packaging and scripts: historical ARMHF qualification sources. These
  are not automatically promoted into NuraLoumi release artifacts.

## Host build

    meson setup /tmp/nuraloumi-browser-engine-native components/nura-browser-engine
    meson compile -C /tmp/nuraloumi-browser-engine-native
    meson test -C /tmp/nuraloumi-browser-engine-native --print-errorlogs

Genuine host development dependencies: WPE WebKit >=2.54.0,
WPEPlatform, GLib/GIO, JSON-GLib. This component has no GTK linkage.
Meson installs the executable only beneath libexec/nuraloumi/browser.
The public NuraLoumi Rust binary is built by Cargo.

## Private control vocabulary

Native URL/query/navigation: open, back, forward, reload, stop, uri, recent.
Native tab commands: tabs, tab-new, tab-select (index), tab-close (index).
Bookmarks: bookmarks (page 0..7), bookmark-toggle.

All commands are strictly allowlisted; only open takes a URL. The Rust
frontend checks the same UID and a private 0600 socket beneath 0700
XDG_RUNTIME_DIR. The engine validates HTTP(S) URL authority/ports/escapes.
No TCP server, shell/eval, or WebKit page API reaches the control channel.

## Release gates

An x86_64 WPE 2.54.2 link and host C tests do not prove:
ARMv7/VFPv3-D16 no-NEON closure, device-safe sandbox, real compositor
pixels, native WPE tab toplevel handoff, or SL101 input. No device promotion
or replacement of the existing Cog path is permitted without separate
qualification and a rollback. The NuraLoumi UI does not automatically launch
an unqualified WPE process.

See ../../docs/architecture/NURA-BROWSER-20261113.md and
../../docs/ux/NURA-BROWSER-20261113.md.

All development and validation is local-only. Do not publish or dispatch
GitHub, external CI, Git network or device operations without explicit approval.

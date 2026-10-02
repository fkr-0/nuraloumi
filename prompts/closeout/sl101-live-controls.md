@projmgrauth
Set repo: nuraloumi

# NuraLoumi SL101 Live Window Controls Qualification

Canonical OCP task: NURALOUMI-SL101-LIVE-CONTROLS-20261002
Priority: 94
Lane: device-window-controls

This is device qualification, not another foreign-toplevel implementation lane. First verify the current shell E2E and Wayland foreign-toplevel packets are completed/reviewed. Do not duplicate them while active.

On the real SL101/Nura:
- run the current copy-only build under the known labwc/pixman recovery session;
- open at least two or three applications/windows;
- verify live windows menu updates on create/destroy/title/state changes;
- focus different windows;
- toggle fullscreen on then off for a controllable window;
- close a window through the confirmation path;
- destroy/recreate windows and prove stale opaque tl:<generation> IDs fail closed;
- verify title/app-id cannot act as control identity;
- verify ext foreign-toplevel-list fallback stays read-only when wlr management is absent;
- capture compositor capability/source information and commands/evidence;
- do not replace the default session or weaken confirmation safety.

Acceptance: evidence-backed PASS/PENDING matrix for list/focus/fullscreen/close/stale-id/ext fallback on actual SL101, with no compositor wedge and rollback preserved.

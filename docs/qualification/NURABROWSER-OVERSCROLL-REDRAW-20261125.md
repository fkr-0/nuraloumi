# NuraBrowser overscroll redraw elimination — host regression

Date: 2026-11-25. Scope: browser chrome gesture-to-Cairo scheduling only.

## Finding

The native browser's lower bookmark/status list uses bounded touch scrolling.
Once a finger passes the swipe threshold, `BrowserScrollDrag::motion` returns a
clamped offset. Previously the event loop requested a full Cairo render on
every such motion, including when the offset was already at zero or the maximum
and no pixels could change. Repeated overscroll gestures could therefore
schedule a large number of redundant full-surface redraws on Tegra20.

## Fix and checks

`advance_browser_scroll` now cancels the pending tap as soon as a swipe is
recognized, but only requests a frame if the clamped offset actually changes.
The original wrong-touch-ID, swipe threshold and release rules are retained.

A deterministic binary regression drives **64 top-edge + 64 bottom-edge**
touch motions without requesting any redraw. Moving back into scrollable
content requests a frame. Both edge gestures still suppress tap activation.

The shared Wayland backend already handles a busy two-slot wl_shm pool: on
`WouldBlock` it records `redraw_pending`, and a later `wl_buffer.release`
enqueues `PlatformEvent::Configure`. Browser chrome handles this event and
retries the current scene. No backend change was necessary for that path.

## Boundary

This proves redraw scheduling in the host event model, not real touchscreen
latency, GPU/compositor time, pixel delivery, FPS or thermal improvement.
Device performance and any surface-buffer reuse optimization await a supervised
ARMv7 run with a qualified native runtime. No target mutation or ABC dispatch.

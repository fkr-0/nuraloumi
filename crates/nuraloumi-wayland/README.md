# nuraloumi-wayland

Native client backend for NuraLoumi's software reference path.

## Protocol and crate choices

- wayland-client 0.31.15
- wayland-protocols-wlr 0.3.12 with the client feature
- rustix 1.1.5 for Linux memfd_create/ftruncate
- memmap2 0.9.11 for writable shared mappings
- wlr-layer-shell-unstable-v1, client side only
- no Smithay compositor/server internals
- no EGL, GLES, X11, or XCB dependency

The backend binds wl_compositor, wl_shm, the first wl_seat, all wl_output
globals, and zwlr_layer_shell_v1 when advertised. Missing layer-shell is
reported in BackendCapabilities and surface creation returns an explicit
MissingGlobal error. ARGB8888 and XRGB8888 support is recorded from wl_shm
format events; presenting an unsupported format fails explicitly.

## SHM contract

Frame bytes are native-endian packed 32-bit ARGB8888/XRGB8888, matching
Wayland wl_shm and Cairo ARGB32 conventions on the little-endian SL101.
Stride may exceed width*4; only packed pixel bytes are copied into the Wayland
buffer. Each surface owns exactly two memfd-backed slots. A resize is refused
with WouldBlock while any previous-size slot is compositor-owned, so repeated
configure/resize cannot grow allocation without bound.

The two-slot allocation is atomic: if either memfd/map/buffer allocation fails,
any partially-created slots are destroyed and the next submission retries from
a clean lifecycle state. wl_buffer.release returns a slot to the producer, and
generation tags prevent a late release from an older allocation from freeing a
current slot.

## Surfaces and focus

Panel surfaces use the top layer, anchors top/left/right, configure an
exclusive zone, and set keyboard interactivity to None. The panel therefore
does not steal keyboard focus merely by existing.

Menu/sheet surfaces use the overlay layer, top/left anchoring, margins, and
Exclusive keyboard interactivity while the transient menu is visible.

## Input and outputs

Wayland pointer and touch coordinates are already wl_surface-local logical
coordinates after compositor output transform handling, so they are forwarded
without a second transform. Stable touch IDs are preserved through
down/motion/up and cancel reports all active IDs.

Keyboard events expose arrows, Enter, Escape, Backspace, raw key codes, and a
small US-layout text fallback. This fallback avoids a mandatory xkbcommon
runtime dependency; a shell may layer richer keymap/text handling later.

wl_output scale and transform metadata are tracked. normalize_output_point is
provided for calibration/tests that start with physical output coordinates and
covers normal, 90, 180, 270 and flipped transforms.

## Live demo

The live smoke is intentionally opt-in because CI may not have a compositor.

~~~sh
cargo run -p nuraloumi-wayland --example wayland_shm_demo
~~~

Required compositor globals for rendering:

- wl_compositor
- wl_shm with ARGB8888 or XRGB8888
- zwlr_layer_shell_v1

For the demo's pointer/touch/key reactions, wl_seat must also be advertised.
wl_output is consumed when present for scale/transform metadata. The demo opens
a keyboard-interactive software-rendered menu sheet, paints a visible
checker/gradient pattern, changes the phase on pointer/touch/key input, exits on
Escape or compositor close, and destroys the surface cleanly.

A wlroots/labwc session with WLR_RENDERER=pixman is the intended SL101 baseline.
The current development host may not expose WAYLAND_DISPLAY, so live compositor
qualification is deliberately separate from unit tests; protocol-independent
buffer lifecycle, transform, and key mapping helpers remain host-testable.

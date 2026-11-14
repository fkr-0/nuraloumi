# Browser isolation from the NuraLoumi three-pass player (2026-11-14)

## Finding

The video player uses a SHA-pinned private Grate 0092 renderer in its
child playerd process only. Compiler options GRATE_I420_YU_PAIR=1 and
GRATE_I420_YU_FP_SCHEDULE=1 are attached to the daemon spawn, not the
process-global environment.

The NuraLoumi browser control UI uses Cairo and Wayland wl_shm and does
not require EGL. Its WPE WebKit engine runs independently and uses a
separate renderer selection: software mode selects swrast, while the
Grate mode requires a distinct SHA-pinned WPE overlay. No part of the
browser automatically consumes the new video player private bundle.

For defense in depth, the browser's native launcher now unsets
player-specific three-pass compilation and trace variables before it
chooses its own renderer. The historical public browser alias was
removed intentionally; an old test still demanded it, and is corrected.

## Completed tests

- Browser Rust pointer/touch integration: 2/2 PASS.
- Native browser engine host Meson tests: 7/7 PASS.
- Browser ownership and migration contracts: 7/7 PASS.
- Host browser/video renderer isolation tests: 4/4 PASS.
- Rust browser fixture with and without video Grate flags: identical
  two-tab/two-bookmark headless output.
- Installed browser fixture on physical ARMv7 target: PASS.
- Installed panel environment has no player Grate specialization flags.
- Three-pass integrated NuraLoumi playerd: full 414-frame EOS,
  shared-clock audio and complete Wayland releases, controls PASS.
- No compositor restart, system Mesa replacement, browser deployment,
  WPE sandbox bypass, or new renderer selection performed.

## Explicit limits and recommendation

No live WPE page WebProcess was running on the inspected target.
Consequently physical WebKit page pixel accuracy, video inside a page,
shared GPU contention, four-buffer saturation and WPE/Grate runtime
compatibility have not been qualified by these tests.

Keep the browser on its own renderer path. Do not automatically adopt
the player's shader-specific three-pass Grate build for WPE. A browser
GPU migration requires independent WebProcess launch, EGL import,
shader/pixel, buffer-lifetime and concurrent audio/video qualification.

Public CI now checks source-level browser isolation and basic browser
ownership contracts without requiring private device credentials or
vendor libraries. Native host Meson tests and physical browser
qualification remain independent release procedures.

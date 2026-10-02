# NuraLoumi agent guide

NuraLoumi is a low-resource touch shell/menu suite for the ASUS Eee Pad Slider SL101 ("Nura", Tegra20/ARMv7).

## Product boundary

NuraLoumi is not a desktop environment. It is a compact Wayland-native set of menu surfaces intended to run on top of labwc/wlroots and remain usable with WLR_RENDERER=pixman. GPU acceleration is optional.

Core design:
- DeskHalloumi-inspired renderer-neutral menu semantics and visual tokens.
- modrelease-2-inspired direct Cairo drawing primitives.
- Native Wayland shared-memory presentation; no XWayland requirement.
- 48-52 logical-pixel primary touch targets.
- Physical keyboard parity for the SL101 slider keyboard.
- No NEON assumption; Tegra20 compatibility is a release gate.

## Repository rules

- Read ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, and docs/CONTRACTS.md before implementation.
- Respect crate ownership from docs/AGENT-LANES.md. Do not edit another active lane's crate.
- Keep UI/model/provider boundaries explicit. System commands never execute from render code.
- Software rendering is the reference path. Never make startup require EGL/GLES.
- All external commands must be bounded, testable, and replaceable with fixtures.
- No live desktop/session mutation from unit tests.
- Prefer deterministic scene/layout tests over screenshot-only assertions.
- Preserve unrelated work. Use ws-bridge claims/transactions in concurrent work.
- Before handoff run the narrow crate tests, then cargo test --workspace and cargo clippy --workspace --all-targets -- -D warnings when practical.

## SL101 power-state safety

- Never run unattended `rtcwake -m mem`, force `mem/deep` through `/sys/power/state`, or use `loginctl -i` / `--ignore-inhibitors` on the SL101.
- Do not treat RTC wake as proof of successful resume. Display, input, EC, SDIO/Wi-Fi, compositor, providers, and network recovery must all be checked.
- Real suspend qualification requires a human physically present with a recovery path and an exact recorded kernel + DTB identity.
- Use the inhibitor-aware `loginctl` path. Start with dry-run, then shallow `freeze`; deep suspend is a separately authorized experiment only after shallow resume passes.
- If SSH is the only recovery/control path, do not issue a real suspend.
- NuraLoumi suspend must remain dry-run unless the separate unsafe-suspend capability is explicitly enabled; ordinary reboot/poweroff enablement is not sufficient.
- Read `docs/qualification/SL101-BOOT-SUSPEND-20261002.md` before any SL101 sleep-state work.

## Performance budget

Target device class: dual-core Tegra20, ~1 GiB RAM, ARMv7 without NEON.
- Idle shell goal: <= 35 MiB RSS for NuraLoumi processes combined.
- Menu-open incremental RSS goal: <= 12 MiB.
- First interactive frame goal: <= 250 ms from invocation on target.
- Steady menu interactions: <= 16 ms CPU rendering budget where possible; <= 33 ms acceptable on target software rendering.
- No continuous animation while idle.

# NuraLoumi agent guide

NuraLoumi is a low-resource touch shell/menu suite for the ASUS Eee Pad Slider SL101 ("Nura", Tegra20/ARMv7).

## Owner privacy and local-only development policy

**Local-first development; owner-directed release operations.** Work directly
in the local repository and governed worktrees using local tests, reviews,
commits and merges. An owner request to **release/deploy NuraLoumi to the
known SL101 Nura tablet** includes the ordinary prerequisite of pushing the
exact reviewed release commit to its configured upstream and triggering its
official main-branch CI; do not ask for a second redundant approval for those
release steps. Verify the exact ref, clean tree, destination and absence of
private infrastructure in outgoing content before publishing. Outside that
scoped release request, do not fetch, push, trigger remote CI, create public
PRs or publish files without owner direction. Never interpret this as consent
to publish secrets, unrelated repositories or experimental device images.

**Private infrastructure stays private.** Treat hostnames, IP addresses,
internal paths and URLs, network topology, account details, credentials, and
device identifiers as non-public. Do not put them into public PR bodies,
issues, commit messages, release notes, shared logs, or external tool calls.
Use generic placeholders in text and test fixtures where addresses are
unnecessary. Check outbound content for leaks before any separately
authorized publication.

Preserve unrelated local edits, never reset or clean another agent's state,
and never assume a successful host build authorizes replacing a live SL101
session. Any device operation needs its own verified qualification and
rollback plan.

For owner-designated Nura `.106`, there is no physical serial sticker and no
repeated sticker/photo request. Start with the approved target using the
existing trusted SSH host key; collect SD/eMMC CIDs, model, boot/kernel/DTB,
runtime and session identity read-only. The owner has already confirmed that
physical recovery and a recovery SD are routinely available; do not re-ask
that standing fact. Preserve rollback hashes and a known-good software
renderer before changing the active release.

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

## Execution-first contract: no status-only work and mandatory gates only

- **A status declaration is not a deliverable.** For an owner-requested fix,
  integration, test, deployment or review-with-fixes, do not terminate with
  `BLOCKED`, `NO-GO`, `UNKNOWN`, a readiness matrix, or another prose-only
  diagnosis while an authorized, bounded implementation or test action remains.
  Execute that action and verify its outcome. Accurate status is allowed only
  as evidence accompanying work or when a truly mandatory stop is proven.
- **Default for proposed gates is non-blocking.** A historical checklist,
  agent-authored policy, reviewer preference, speculative storage estimate,
  optional attestation, or missing ideal evidence does not become a prerequisite
  merely because a worker called it a gate. First attempt a safe, reversible
  route within the task's actual scope; downgrade unverified gates to follow-up
  diagnostics rather than recursively producing NO-GO reports.
- **Burden of proof for a mandatory stop:** identify (1) the exact current
  authority: explicit owner constraint, active repository safety rule, actual
  permission/security boundary or hardware-damage risk, or required executable
  dependency; (2) the precise operation it forbids; (3) current first-hand
  evidence or an attempted negative test; and (4) why no safe in-scope variant
  can continue. A stale worker assertion or a hypothetical failure is not
  proof. If any element is missing, do not stop the rest of the work.
- **Distinguish test from release.** `UNQUALIFIED` and
  `device_deployment=FORBIDDEN` on a candidate forbid promoting it into
  production, changing active boot/session/release state or claiming a
  qualified PASS. They do not, by themselves, forbid an explicitly authorized,
  isolated, temporary private device smoke test with verified target identity,
  bounded duration/resources, independent logs, cleanup and rollback. Do not
  change the marker to manufacture a release.
- **Use real observations, not ceremonial provenance.** For an authorized
  60-second browser video smoke, prefer a genuine bounded playback attempt,
  browser/media logs and compositor-frame evidence. An extra signed physical
  touch-event custodian, protected nonce service, complete rebuild-peak audit
  or exact hardware-input source is optional unless the actual test objective
  or a verified mandatory authority requires it. Label synthetic/automated
  input honestly; never misrepresent it as physical touch.
- **After a failure, fix or narrow immediately.** Reuse known CI artifacts,
  repair the smallest broken runtime/packaging/launch path, rerun the failing
  step and preserve positive and falsifying evidence. Continue independent
  work when one step truly blocks. A relength child is justified by a concrete
  runnable fix and acceptance test, not by a restatement of the prior status.
- **Non-negotiable protections stay intact:** real permission denials, owner
  consent boundaries, production/boot/suspend safety, device identity,
  preservation of live Sway/WayVNC and existing releases, private credentials,
  exclusive ownership, resource/thermal aborts and rollback requirements must
  never be bypassed for a nominal test success.


## NuraBrowser media CI reuse-first rule

- **Inspect existing producer CI before rebuilding.** Inventory local Docker images/layers, build trees, caches, WPE runtime packages, ELF artifacts, CI logs and manifest hashes. Distinguish WPE 2.54.0-2 from 2.54.2 and glibc from ARMv7-musl; never assert compatibility from an image tag alone.
- Prefer a verifiably compatible existing build or incremental continuation. Preserve source and build caches; do not require a fresh upstream tarball if existing independently traceable binaries satisfy the task. Missing original archive is a rebuild-input gap, **not automatically a runtime candidate NO-GO**.
- Disk capacity is an operational measurement, not a hypothetical policy barrier: inspect actual `df`, `du`, active build footprint, layer reuse and writable upper-layer growth. A historic 20/40-GiB estimate is **not a measured lower bound**. Require a positive practical reserve and monitor usage, but do not demand an authenticated full fresh peak trace before a small, bounded, reversible stage. Avoid unchecked full rebuilds on a nearly full root.
- Keep distinct gates: (1) reuse/build feasibility, (2) artifact ABI/loader/media plugin qualification, (3) device Play/pixel safety. Do not let a missing full-rebuild precondition veto inspection or reuse. Do not fabricate ARM/factory/device PASS; evidence remains required at the gate actually exercised.
- No Docker prune, image/volume/cache deletion, cleanup of shared checkouts or active workers without exact owner authorization. Claims, worktrees and user changes remain protected.
- For a blocked task, first name a concrete runnable reuse/continuation path and the exact missing observation. Avoid recursive NO-GO reports, speculative capacity arithmetic or repeated source-custody bureaucracy that does not advance the user's 60s playback goal.

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

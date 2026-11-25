# SL101 RC2 deployment gate — independent audit (2026-11-25)

**Decision: NO-GO** for activating RC2 or claiming complete GPU-video/browser qualification. This is a host-side, read-only audit; no SL101 mutation was performed.

## Identity and source alignment

- Actual starting checkout HEAD at first inspection: `c415f681cd0d006a5fb6b3045e840edea87b58bc` on `main`, ahead of `origin/main` by four commits; dirty unrelated `crates/nuraloumi-shell/src/lib.rs` and two prompt paths. Other concurrent agents advanced the branch during this audit; later observed HEAD `65ddad6ad61d737167238b2b04fe5524e3bb1402`, six commits ahead. None of these later SHAs inherits the old CI evidence.
- UI fix is ancestor `95bb56bc20033cb9db6b2d1570af18598d86dcfe`. The official RC2 release branch and `origin/main` currently point to `623609cbe4413af3313682b9478bb505a883510d` (pre-UI source).
- Official push CI at that exact source: Alpine/musl run `45545537613`, completed success (2026-11-25 04:22:33 UTC); GitHub API reports one successful ARMv7 job. Companion CI `45545537739`, Debian `45545537582`, and WPE ARMHF build workflow `45545537470` report success, but **workflow success does not qualify an installed musl WPE runtime or GPU bundle**.
- `gh api repos/fkr-0/nuraloumi/actions/runs/45545537613/artifacts` returned exactly `nuraloumi-musl-armv7-build-audit-54-623609cbe4413af3313682b9478bb505a883510d` (3,763 bytes, not expired). Its workflow `.github/workflows/sl101-musl-armv7.yml` explicitly uploads only `BUILD-LOG.txt`, `BINARY-LIST.txt`, `ELF-AUDIT.txt` on push. No executable ELF, release tar/APK, `runtime.json`, GPU producer package or deployable browser comes from that run. The three text members were established by workflow contract; this audit did **not** independently download and hash the ZIP members. A successful job is narrower than a verified binary archive.
- A `target/armv7-unknown-linux-musleabihf/release/` local binary tree exists, but its source identity and archive chain are not a replacement for official producer outputs. Historical tarballs under `/home/user/testbins/nuraloumi` are from other revisions (e.g. `df6182b5`) and are **not** promoted as source-matched RC2 artifacts.
- The installed `/opt` release is described in the task handoff as 28 no-NEON musl executables with RC1 private `lib/config/share` assets copied byte-identically. That is handoff evidence, **not remotely re-verified installed hashes or an official complete-video artifact**; the precise current symlink, processes, hashes and boot/session identity remain UNKNOWN while SSH is denied. The installed player was not qualified. The WPE musl headless candidate reportedly crashed and stays unqualified; no device-side reproduction occurred in this audit.

## GPU private bundle source/ABI/loader gate

The normative source contract is `docs/qualification/SL101-GPU-BUNDLE-CI.md`, implemented by `scripts/verify_sl101_video_bundle.py`, `scripts/verify_packaged_sl101_video.py` and the dispatch-only GPU branch of `.github/workflows/sl101-musl-armv7.yml`.

1. Producer `sl101_linux` must publish `sl101-gpu-video-bundle` with a single `video-bundle.tar.gz`, pinned producing run ID, 40-character source revision and independently supplied SHA256. Archive members must be regular, safe, exact and mapped in `provenance.json` with schema `sl101-gpu-video-bundle-v1`, target `armv7-unknown-linux-musleabihf`, ABI `armv7-musleabihf-vfpv3-d16-no-neon-no-idiv`, and per-ELF SHA256.
2. Required chain: `lib/libnuraloumi-video.so`, `lib/libgallium-25.0.7.so`, `lib/libEGL.so.1`, `lib/libGLESv2.so.2`, `lib/libgbm.so.1`, `lib/libdrm.so.2`, `lib/libdrm_tegra.so.0`, `gst/libgstv4l2codecs.so`, **plus every** private transitive ELF/GStreamer library and plugin. The specific `libgallium-25.0.7.so` contract is version-sensitive: the separate device preflight documents `libgallium-26.2.4.so`; this cannot be silently treated as the required producer version.
3. Verifier checks ELF32 ARM machine, hard-float ABI, VFPv3-D16, no NEON/IDIV via independent instruction audit, proper musl dependency (never `libc.so.6`), forbidden RPATH/RUNPATH, duplicate exported SONAMEs, and `DT_NEEDED` closure against the supplied SONAME set plus explicitly allowed musl system libraries. This is a static dependency and code-policy check, not proof of runtime plugin loading, VDE operation or safe device pixels.
4. Consumer must retain the entire producer `lib/` and `gst/` closure and pass `verify_packaged_sl101_video.py` with exact NuraLoumi source SHA, release version, producer revision, module hashes, archive manifest and no dropped/injected modules. Only explicit `workflow_dispatch` with exact WPE-runtime inputs can promote the regular package; the GPU variant additionally needs three exact GPU producer inputs.
5. **Observed missing chain:** the latest ordinary push artifact contains text audit only, hence no `provenance.json`, archive SHA, producer run ID, independently inspected SONAME graph, private plugin tree, exact-source binary release tar, browser `runtime.json`, WPE process/sandbox closure or device playback proof. Copying private files from RC1 into a local RC2 directory establishes neither producer integrity nor complete dependency closure.

Tests: `python3 -m unittest scripts.test_verify_sl101_video_bundle scripts.test_verify_packaged_sl101_video -v` → exit 0, **18/18** tests passed (including cross-libc, missing plugin, unclosed dependency, tampering, duplicates, traversal and symlink negatives). These are contract/unit tests, not an audit of real producer bytes.

## SSH/network gate (non-mutating)

- Command: `ssh -o BatchMode=yes -o ConnectTimeout=6 -o ConnectionAttempts=1 -o NumberOfPasswordPrompts=0 -o StrictHostKeyChecking=yes user@SL101 true`
- Result: exit **255**, `Permission denied (publickey,password)`. OpenSSH also warned about non-post-quantum KEX. The server was reachable and responded through SSH authentication; **no remote shell command ran**. There was no password prompt, host-key replacement or policy change.
- `ssh -G SL101` showed effective alias host `sl101`, port 22, user `user` (exit 0); this is configuration evidence, not account/key acceptance evidence.
- Safe recovery: have the owner use the already available physical console/recovery SD or another **existing authorized administrative session** to inspect SSH server/authentication logs, account/key eligibility and the exact boot/device identity read-only. Confirm the authorized login path and trusted host key out of band. Retry one bounded publickey login only after that finding. **Do not** disable checking, change keys, allow password automation, alter `sshd_config`, or restart a service in this task.

## Go/no-go and rollback decision

| Gate | Status | Required independent proof |
| --- | --- | --- |
| Exact UI source and official full binary artifact | **NO-GO** | official successful CI for `95bb56bc`-descendant exact release SHA, downloaded binaries plus source/manifests and recomputed hashes; current successful push is pre-UI and text-only |
| ARMv7/musl ABI/ISA | **PARTIAL** | independently inspect actual official ELF objects, loader, `DT_NEEDED` chain, and no-NEON/no-IDIV rather than only text audit |
| GPU producer and private closure | **NO-GO** | source/run/archive SHA, provenance and all ELF/SONAME/plugin checks, packaged GPU artifact recheck |
| Native WPE musl browser | **NO-GO** | qualified same-SHA WPE producer runtime, sandbox, WebProcess/NetworkProcess/dlopen/module closure and no crash |
| Authenticated device inspection | **BLOCKED** | SSH authentication or supervised console read-only evidence |
| Hardware video/player | **UNTESTED** | private loader/plugin test, correct VDE DMA-BUF, 854×480 24fps / 414-frame EOS, AAC/audio sync, no SIGILL, repeatability and resource measurements |
| Browser on SL101 | **UNTESTED** | launch/navigation/tabs, pixels via WayVNC screenshot, focus/touch, sandbox processes, repeated crash/restart recovery |
| Activation | **NO-GO** | independent above PASS records, owner sign-off, device rollback evidence |

For the eventual separately authorized canary, first record current target symlink/target tree and every relevant SHA256, preserve a known-good Pixman/software-rendered fallback and rollback archive, and verify a distinct inactive versioned stage. Do **not** switch `/opt/nuraloumi/current` or restart Sway/playerd while any gate is open. Rollback plan: on qualified canary failure, restore the exact recorded previous symlink and matching library/config tree using the owner's guarded change procedure, verify hashes and software UI, then restart only explicitly targeted user services as separately authorized. At present the existing target cannot be inventoried because login fails, so even rollback **readiness is unproven**.

## Reproduction record

- `git rev-parse HEAD` at start → `c415f681cd0d006a5fb6b3045e840edea87b58bc`, exit 0; later → `65ddad6ad61d737167238b2b04fe5524e3bb1402`, exit 0. Concurrent edits preserved.
- `gh run list --limit 12 --json databaseId,headSha,headBranch,conclusion,status,workflowName,event` → exit 0; recorded official runs above.
- `gh api repos/fkr-0/nuraloumi/actions/runs/45545537613/artifacts --jq '.artifacts[] | [.name,.size_in_bytes,.expired] | @tsv'` → exit 0; one text-audit artifact.
- `gh run view 45545537613 --json headSha,conclusion,jobs` → exit 0; matching source and successful ARM job.
- `python3 -m unittest scripts.test_verify_sl101_video_bundle scripts.test_verify_packaged_sl101_video -v` → exit 0, 18 passed.
- Bounded SSH command → exit 255, authenticated login denied.
- No GitHub mutation, no device install, no service restart, no reboot, no tag, no push.

**Conclusion:** current CI green is real but too narrow. Keep current installed state unchanged and require a fully source-matched binary plus qualified private video and WPE outputs, then an authenticated device canary and independently checked rollback before promotion.

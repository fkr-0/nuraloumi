# SL101 ↔ NuraLoumi system landscape — 25 November 2026

This document distinguishes (1) the running postmarketOS/musl tablet, (2) privately staged VDE/GPU/WPE assets, (3) local source and CI, and (4) the separate Debian/glibc experiment. A source file or host test is not an installed/qualified device component.

## 1. Hardware → kernel → compositor → application surfaces

```mermaid
flowchart TB
  subgraph SOC["ASUS SL101 / Tegra 2 SoC"]
    CPU["Dual Cortex-A9 ARMv7 hard-float<br/>VFPv3-D16; no NEON / no ARM IDIV"]
    DEC["Tegra VDE video decoder"]
    G3D["Tegra GR3D GPU"]
    DC["Tegra display controller / panel"]
  end

  subgraph KERNEL["postmarketOS Nura: Linux 7.0.1 grate/dmavde branch"]
    V4L2["V4L2 stateless decoder / media controls"]
    DRM["Tegra DRM/KMS / render node<br/>/dev/dri/renderD128"]
    AUDIO["ALSA / Tegra audio device"]
    INPUT["evdev/udev: keyboard, touch, candidate slider switch"]
  end

  subgraph USER["ARMv7/musl session: Sway + NuraLoumi"]
    SWAY["Sway / wlroots compositor<br/>Wayland + layer-shell + DMA-BUF"]
    UI["NuraLoumi panel + menu + Control Center<br/>Cairo wl_shm surfaces"]
    PLAYER["NuraLoumi player + playerd<br/>single native video session"]
    WEB["NuraBrowser chrome + private WPE engine"]
    NOTIFY["Notification daemon / history / toast / slider observer"]
  end

  DEC --> V4L2
  G3D --> DRM
  DRM --> SWAY
  SWAY --> DC
  CPU --> USER
  INPUT --> SWAY
  AUDIO --> PLAYER
  V4L2 --> PLAYER
  PLAYER --> SWAY
  WEB --> SWAY
  UI --> SWAY
  NOTIFY --> UI
```

- Sway owns DRM/KMS display composition and modesetting. The native player renders into Wayland-backed buffers; it must not become DRM master or replace Sway.
- Cairo panel/menu text and icon rendering is intentionally independent of video rendering. GPU-video playback does not make the whole desktop GPU-accelerated.
- The actual compositor renderer may select Grate/GLES2 or its safe Pixman fallback; a green desktop screenshot alone cannot establish the renderer or 3D/VDE activity.
- The kernel and EGL/GL userspace are distinct gates. CPU D16-only support restricts both prebuilt shared libraries and JIT/generated instructions (including Mesa/LLVM software paths).

## 2. Native video and audio path — desired private production bundle

```mermaid
flowchart TB
  FILE["MP4: qualified 854×480, H.264 24 fps + AAC"]
  DEMUX["GStreamer 1.x<br/>filesrc → qtdemux → h264parse"]
  VDEC["v4l2slh264dec<br/>private patched v4l2codecs plugin"]
  VDE["Kernel stateless V4L2 → Tegra VDE"]
  BUFS["Three-plane I420 / YU12 DMA-BUF<br/>strides and BT.709 checked"]
  EGL["GBM / EGLImage import<br/>private EGL/GLES/DRM closure"]
  GRATE["Grate Gallium GLES2<br/>four internal passes; YUV→RGB shader"]
  WLC["Wayland buffer commit + frame callback + release"]
  SWAY["Sway compositor → DRM/KMS scanout"]
  PANEL["SL101 display panel"]

  FILE --> DEMUX --> VDEC --> VDE --> BUFS --> EGL --> GRATE --> WLC --> SWAY --> PANEL

  DEMUX --> AAC["aacparse + avdec_aac"]
  AAC --> CLOCK["audioconvert / audioresample / volume"]
  CLOCK --> PA["PulseAudio sink → ALSA hardware"]
```

| Element | Provenance or gate | State |
| --- | --- | --- |
| Decoder | Tegra VDE stateless V4L2 with private source-patched GStreamer plugin | Private 414-frame stage succeeded historically |
| Buffer transport | DMA-BUF YU12/I420; no CPU YUV map/conversion | Historical stage observed callbacks/releases |
| GPU | Private Grate 0092/0093 lineage, Gallium/Mesa 25.0.7-related producer; system Mesa on device may be 26.2.4 | Version/SONAME and loader provenance must be rechecked on release |
| Audio | AAC into PulseAudio/ALSA; shared pipeline clock | Pipeline EOS proven in historical stage; acoustic lip-sync not |
| Integrated UI | One nuraloumi-player and one nuraloumi-playerd, video-bridge library dynamically loaded | Installed RC2 shows generic failure; precise host-side diagnostics added in 9b9284b |
| Acceptance | 414 frames, EOS, frame callbacks/releases, 24 fps, Pause/Seek/Stop, no SIGILL, sustained memory/thermal | Current installed production player NOT qualified |

The old 414-frame result was from a private no-install stage, not proof that today's /opt/nuraloumi/current loads an identical private runtime. The ordinary musl Rust binary audit does not bundle this dependency tree.

## 3. Browser path — separate WPE/musl process tree

```mermaid
flowchart TB
  CHROME["NuraLoumi browser UI / tabs / URL / history"]
  IPC["Native browser IPC + supervisor"]
  CORE["nurabrowser-core: WPE WebKit 2.54.2<br/>ARMv7/musl candidate"]
  SANDBOX["bubblewrap + xdg-dbus-proxy<br/>user/mount/network namespace contract"]
  WEBPROC["WPEWebProcess"]
  NETPROC["WPENetworkProcess"]
  WPEPLAT["WPEPlatform Wayland / EGL / render backend"]
  SWAY["Sway Wayland compositor"]
  DISPLAY["DRM/KMS display"]
  DEPS["Private musl closure<br/>libc, ICU, GIO, crypto, EGL/Mesa, dlopen modules"]
  ISA["Tegra2 ISA gate<br/>VFPv3-D16, no NEON/IDIV, no D32 generated code"]

  CHROME --> IPC --> CORE
  CORE --> SANDBOX --> WEBPROC
  SANDBOX --> NETPROC
  CORE --> WPEPLAT --> SWAY --> DISPLAY
  DEPS --> CORE
  DEPS --> WEBPROC
  ISA --> DEPS
```

- This WPE runtime is **not** the VDE/GStreamer player. The browser activation code currently forces a software GL path for its private engine (softpipe/swrast); that is not proof of working or accelerated video.
- A local Docker/Ninja build produced a candidate source-pinned to WPE 2.54.2. The D16 crypto overlay removed known ARM ISA risks in selected libraries, but complete decoded ARM/Thumb coverage, dlopen closure, sandbox, WebProcess lifetime and target pixels remain open.
- The first private headless attempts can launch or load some processes yet fail later; an about:blank engine start is not a rendered browser PASS.
- Any LLVM/JIT anonymous-code SIGILL on Cortex-A9 must be traced to the actual mapped object and instruction; do not assume ELF attributes alone prove D16 safety.
- A separate Debian ARMHF/glibc build uses a different ELF interpreter and libc closure. It can inform source portability, but its shared libraries/process helpers cannot be placed inside the musl runtime.

## 4. Session surfaces, notifications and physical inputs

```mermaid
flowchart TB
  KEYS["Sway keyboard / touch / Super shortcuts"]
  SWAY["Sway bindings and workspace state"]
  PANEL["nuraloumi-panel: IPC owner + status bar"]
  MENU["Launcher / apps / spaces / tasks<br/>scroll and safe hit regions"]
  CC["Control Center / five category icons"]
  CLI["nuraloumi-ctl notify or D-Bus notificationd"]
  HISTORY["Notification history / center"]
  TOAST["nuraloumi-toast<br/>one-shot floating Wayland surface"]
  SENSOR["SL101 slider candidate evdev/sysfs signal"]
  OBS["Opt-in debounced slider observer"]

  KEYS --> SWAY --> PANEL
  PANEL --> MENU
  PANEL --> CC
  CLI --> PANEL
  CLI --> HISTORY
  PANEL --> TOAST
  SENSOR --> OBS --> CLI
```

- The menu placement, scroll and font-free category-icon changes have host tests. Device pixels and touch remain unqualified until a source-matched build is installed.
- A notification may be recorded in history even when toast spawning or layer-shell visibility fails. Source fix a0f4287 stops treating unknown notification-center visibility as an automatic toast suppression and adds lifecycle diagnostics; a real floating banner is not yet observed.
- Terminess Nerd Font 3.5.1 candidate covers 35/35 listed Nerd glyphs locally, but its exact-file renderer, license/asset packaging and real panel path are not integrated. Font-free Cairo icons remain the reliable control fallback.
- Slider observer is opt-in, debounced, and does not notify on initial snapshot. The physical switch identity and open/closed polarity have not yet been measured on the SL101.
- Super+Shift+R previously reloaded Sway and invoked a panel wrapper through exec_always; the source-level restart fix lives in sibling sl101_linux, not the media engine. Installed behavior still needs a supervised canary.

## 5. Repositories, ABIs, producers and release gate

```mermaid
flowchart TB
  NURA["nuraloumi main<br/>Rust UI, C player bridge, WPE engine wrapper"]
  SL["sl101_linux<br/>kernel, device config, Grate/Mesa/VDE/GStreamer patches"]
  PUSH["GitHub push CI<br/>28-ish ARMv7/musl Rust ELF audit"]
  WPE["WPE 2.54.2 musl producer<br/>currently UNQUALIFIED"]
  GPU["SL101 private GPU-video producer<br/>revision + run ID + archive SHA"]
  CHECK["Same-commit manifest, SHA, ISA,<br/>DT_NEEDED + dlopen + sandbox gates"]
  TAR["Manual package workflow_dispatch<br/>tar + development-signed APK"]
  DEVICE["Versioned staging on .106<br/>rollback + supervised physical qualification"]
  DEBIAN["Separate Debian ARMHF/glibc<br/>.100 / experimental target"]
  NO["NO-GO until required producers and device PASS"]

  NURA --> PUSH
  NURA --> WPE
  SL --> GPU
  WPE --> CHECK
  GPU --> CHECK
  PUSH --> CHECK
  CHECK --> TAR --> DEVICE
  CHECK --> NO
  SL --> DEBIAN
```

**Two release lanes must never be conflated:**

| Lane | Architecture/libc | Package inputs | What it currently proves |
| --- | --- | --- | --- |
| Nura / .106 | ARMv7 hard-float musl, VFPv3-D16 | NuraLoumi binaries + same-SHA WPE runtime + private GPU-video closure | Official pre-UI CI push 623609c is green for 28 text-audited binaries; no complete runtime |
| Debian armhf | ARMv7 hard-float glibc | Debian-specific sysroot, interpreter, WPE/process helpers, appropriate Grate/GStreamer | Separate ABI experiments, not a replacement musl package |
| Host / Docker | x86-64 tools + ARMv7 QEMU | Cross-link and private producer candidates | Source/ELF/host tests, not physical KMS/VDE/Wayland qualification |

**Gate sequence:** exact Git SHA and producer revisions → download official artifacts → independently hash actual bytes → reject wrong libc/ISA/SONAME/RPATH/transitive dlopen → verify WPE sandbox and VDE/GPU on the physical tablet in a versioned inactive stage → record rollback hashes → only then activate an RC. A push artifact marked AUDIT-ONLY-NO-DEPLOYMENT is never a release tar or device-qualification PASS.

## Key evidence

- docs/qualification/SL101-NURALOUMI-PROGRESS-20261125.md — dated status, commits, tests and blockers.
- docs/qualification/SL101-RC2-DEPLOY-GATE-20261125.md — official CI/source-matching and SSH no-go.
- docs/qualification/NURABROWSER-LOCAL-MUSL-CANDIDATE-20261125.md — private WPE/crypto source and ISA preflight.
- docs/qualification/NURALOUMI-PLAYER-E2E.md — historical private 414-frame VDE/GPU integration.
- docs/qualification/SL101-GPU-BUNDLE-CI.md — GPU producer and package contract.
- docs/qualification/MUSL-GITHUB-CI.md — updated push versus release artifact rules.
- docs/qualification/SL101-PANEL-RESTART-20261125.md — Sway restart source and supervised canary.

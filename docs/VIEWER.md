# NuraLoumi generic viewer

The standalone `nuraloumi-viewer` displays PNG and JPEG files in a software
Cairo/`wl_shm` layer-shell surface. Unsupported or unreadable inputs instead
show a **text diagnostic panel**, including output from a bounded `file(1)`
helper. It does not execute, interpret or dump the selected file as a program.

## Use

```sh
# Interactive Wayland surface; PATH is passed as one argv element.
nuraloumi-viewer --live -- "/home/user/Pictures/photo.jpg"
nuraloumi-viewer --live -- "/home/user/Downloads/-unusual.png"

# Headless check: still rasterizes the same Cairo scene, then prints a summary.
nuraloumi-viewer --fixture -- "/home/user/Pictures/photo.jpg"
cargo run --locked -p nuraloumi-shell --bin nuraloumi-viewer -- --fixture Cargo.toml
```

Default invocation is live; `--fixture` requires neither Wayland nor a display.
Fixture mode **does not create a PNG file**, but it does rasterize and validate
a nonblank 800×480 ARGB scene. A diagnostic scene counts as a successful
rendering, even if the file was not readable as an image.

The file manager launches `["nuraloumi-viewer", "--live", PATH]` using a
typed argv operation. The opener also has an Open With selection path when a
suitable application is not available. Filenames, including non-UTF-8 names
and names beginning with `-`, are passed as a single opaque argument.
The `--` CLI delimiter is recommended for manual invocations.

### Controls

- **Esc**, **Backspace** or **Left**: close.
- **Pointer primary button**: press and release inside the Back or X target.
- **Touch**: press and release the *same* contact inside Back or X. Moving
  outside or cancellation prevents activation.
- Image fit preserves the aspect ratio. Output is clipped to the image viewport,
  including for extremely narrow/tall images.

## Decode and diagnostic boundaries

| Property | Effective bound / behavior |
| --- | --- |
| Formats | PNG and JPEG, identified by magic bytes rather than extensions |
| Maximum encoded image input | 12 MiB; a 12 MiB + 1 bounded read detects excess |
| Maximum width/height | 4096 pixels each |
| Maximum decoded pixel count | 4,000,000 |
| Converted image format | Premultiplied BGRA bytes for little-endian Cairo ARGB32 |
| EXIF | JPEG APP1/TIFF orientation values 1–8, including mirror and rotation |
| Metadata parsing | Bounded segment/IFD parsing, no recursive TIFF traversal |
| Maximum rendered framebuffer | 32 MiB; invalid dimensions/scales fail closed |
| Unsupported/unreadable input | Text explaining the reason and bounded `file -- PATH` output |
| `file(1)` output | At most 4096 bytes each from stdout and stderr; controls sanitized |
| `file(1)` execution | Exact argv, no shell; 2-second deadline; isolated process group |
| Rendering backend | Cairo + `wl_shm`; no EGL/GLES, XWayland or NEON requirement |

The helper uses nonblocking reads, so a child process inheriting its pipes
cannot make the viewer wait forever for EOF. On timeout it terminates the
helper's process group, and reaping does not block the caller. Other arbitrary
unrelated subprocesses outside that process group are not controlled.

File reading, decoding, EXIF transformation and `file(1)` identification run
on a **single background worker** after the Wayland surface has been created.
Until the worker completes, the event loop remains responsive and a loading
scene can be presented; there is no idle animation or perpetual polling after
completion. The loading task is not cancellable once its I/O starts, but
closing the UI drops the result receiver.

### ARMv7 performance and memory

Pixel conversion traverses prevalidated source rows/columns instead of
performing integer divide/modulo for every pixel. This avoids a significant
software-operation cost on Tegra20, which lacks hardware integer division.
EXIF rotation still requires a complete destination image; Cairo frame
creation and `wl_shm` presentation can also duplicate buffers. The 4 MP
decoded-image cap is **not** a total-process RSS limit: worst-case peak memory
can exceed the normal 12 MiB menu-increment goal. This viewer is a
standalone, on-demand surface rather than an always-resident menu.

The 250 ms first-interactive-frame target, actual CPU latency, peak RSS and
input interaction still require on-device measurement. A passing host test
does not qualify the SL101 release.

## Verification

```sh
cargo test --locked -p nuraloumi-shell --bin nuraloumi-viewer
cargo clippy --locked -p nuraloumi-shell --bin nuraloumi-viewer -- -D warnings
cargo fmt --all -- --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
```

Focused regression coverage includes real PNG/JPEG decoding, malformed
headers/hostile dimensions, orientation 1–8 and malformed EXIF, extreme image
aspect ratios, Cairo source pixels, asynchronous loading/first-frame
separation, exact argv and untrusted filenames, missing `file(1)`, and a
helper child that exits while a descendant retains stdout/stderr.

For live qualification under Sway test both mouse and touchscreen, small
outputs / non-default scale, corrupt and over-limit images, a 4 MP JPEG,
EXIF-rotated photos, and release-inside/back navigation; collect elapsed
initial frame and peak process RSS. Do not claim device qualification without
those measurements.

See [viewer qualification and review](qualification/NURALOUMI-VIEWER-20261110.md).

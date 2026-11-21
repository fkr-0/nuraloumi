# Cairo/Fontconfig terminal-lifetime diagnostic (host only)

- OCP: `NURALOUMI-FIX-CLOSURE-20261121-06`
- Source HEAD: `fd34f43e6dc27898047c1d95a2cc8748ff8128c7`
- Host: Cairo 1.18.6, Fontconfig 2.18.3, FreeType 26.7.20, clang LSan.

Reproduce from the repository root (each invocation is a distinct process):

```sh
clang -g -O0 -fsanitize=leak \
  crates/nuraloumi-render-cairo/tests/fixtures/cairo_font_lifetime.c \
  -o /tmp/nuraloumi-font06-lsan $(pkg-config --cflags --libs cairo)
/tmp/nuraloumi-font06-lsan no-reset
/tmp/nuraloumi-font06-lsan terminal-reset
/tmp/nuraloumi-font06-lsan owner-order
```

| Mode | Observed exit | LSan | Result |
| --- | ---: | --- | --- |
| `no-reset` | 23 | 357 bytes, 5 allocations | expected negative control |
| `terminal-reset` | 0 | no leak detected | PASS |
| `owner-order` | 0 | no leak detected | PASS |

The negative stack originates from `cairo_select_font_face` ->
`_cairo_toy_font_face_create_impl_face` -> `_cairo_ft_font_face_create_for_pattern`
-> `FcPatternDuplicate` (`libfontconfig.so.1`); the direct allocation is
256 bytes, indirect allocations 96 + 5 bytes. This is **library-scoped static
cleanup**, not demonstrated to be an unfreed NuraLoumi application-owned face.
The positive variants destroy all Cairo objects before the process-terminal
`cairo_debug_reset_static_data()`; `owner-order` also drops an additional
retained Cairo font-face reference first. These controls do **not** establish
that resetting Cairo is safe while other clients are active.

Application attribution: the optional `PackagedFontText` path binds an exact
font file through FreeType without Fontconfig family lookup. Its `FtOwner`
is shared with Cairo's font-face user data, and `FT_Done_Face` precedes
`FT_Done_FreeType` in its destructor. The host reproduction above intentionally
uses Cairo's toy-font path to isolate the reported Fontconfig residual; it
does not exercise the NuraLoumi packaged-font path. Product leak absence has
**not** been established under an instrumented full-process lifecycle.

Checks at this HEAD:

- `cargo test -p nuraloumi-render-cairo --all-features`: exit 0, PASS.
- `cargo fmt --all -- --check`: exit 0, PASS.
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0, PASS.

Classification: **PROVEN-NONBLOCKER** for the reported 357-byte Cairo toy-font
LSan residual; **PENDING** for a separate, product-specific leak claim (none
reproduced here). No global reset was added to any NuraLoumi runtime, and no
hardware, compositor, network or deployment qualification was attempted.

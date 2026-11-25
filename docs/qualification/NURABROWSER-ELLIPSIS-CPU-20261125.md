# NuraBrowser linear-time ellipsizing — host parity gate

Date: 2026-11-25. Scope: deterministic browser scene text truncation.

## Finding

Every Cairo scene build measures toolbar and list labels through
`browser::scene::bounded_text`. Previously the shortening loop built a new
`prefix + character + ellipsis` string at each step and remeasured that whole
prefix. For a clipped label of N characters, this does quadratic cumulative
character scanning and repeated temporary allocations. Long URLs can reach
this path on frequent touch-driven redraws.

`DeterministicText::run_width` is exactly the sum of per-character advances.
The new loop retains the full-label fast path, then accumulates character
widths and appends at most one ellipsis in a single pass. No new dependency,
renderer backend or string sanitization policy is introduced.

## Evidence and boundary

A frozen copy of the previous ellipsizing algorithm is the test oracle. The
new implementation produces identical returned strings across empty, narrow
and wide ASCII, URL, right-to-left, emoji, combining-mark and CJK samples,
including each character/ellipsis width boundary with values just below,
equal to and just above it, for regular and bold styles.

The optimization changes the deterministic algorithmic cost from O(N^2) to
O(N) for clipped labels, and eliminates its per-character temporary `String`
allocations. This is a source- and test-backed complexity reduction, **not**
a measured ARMv7 time, energy or WayVNC pixel speedup. Native stress testing
and renderer-buffer pressure remain separate qualification work. No target
mutation or ABC dispatch.

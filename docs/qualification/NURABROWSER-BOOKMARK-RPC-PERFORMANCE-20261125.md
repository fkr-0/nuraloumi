# NuraBrowser hidden-bookmark RPC reduction — host gate

Date: 2026-11-25. Scope: native browser chrome model; no device or fleet mutation.

## Finding and change

The chrome polls the native WPE engine at 350 ms while loading and 1,500 ms
when idle, to observe in-page navigation even without a chrome Wayland event.
Before this patch every poll issued three sequential same-UID IPC commands:
`uri`, `tabs` and `bookmarks`, then decoded all three JSON responses. The
bookmark list is not drawn when collapsed, yet its full page was still fetched.

The browser now polls `uri` and `tabs` only while bookmarks are collapsed.
The current-page bookmark star still comes from the `uri` response. Expanding
bookmarks sets `bookmarks_open` before refresh; that refresh retrieves and
validates the actual list. Paging also refreshes the list; hiding and later
reopening it never relies on a previously cached list for the first display.
Bounds and offset validation remain mandatory whenever bookmark rows are
visible. Failed expansion restores the collapsed state so cached rows cannot
masquerade as fresh; failed paging restores the previous page to keep its
cached offset consistent. A visible list still receives all three IPC queries
on each poll.

## Deterministic request-count regression

The focused browser-library test simulates 64 successive hidden-state polls:

| Operation | `uri` calls | `tabs` calls | `bookmarks` calls |
| --- | ---: | ---: | ---: |
| 64 hidden refreshes | 64 | 64 | 0 |
| Expand | +1 | +1 | +1 |
| Hide; 64 hidden refreshes | +64 | +64 | +0 |
| Reopen | +1 | +1 | +1 |

For 64 unchanged hidden polls this is **128 instead of 192 command calls**
(33.3% fewer) while retaining current-page star and navigation updates.
In a static idle session it eliminates approximately **40 bookmark RPCs per
minute**, versus the previous code. While loading it avoids approximately
171 bookmark RPCs/minute if that loading state is sustained. These are
event-loop and mock transport counts, **not** measured target CPU, memory,
latency, power or WayVNC performance data. Poll wakeups remain unchanged.

## Qualification boundary

Host tests cover the hidden/open/hide/reopen transition, failed expansion and
paging rollback, existing bookmark bounds, external WebKit navigation, browser
binary behavior, scene layout and supervisor lifecycle. Native ARMv7/WebKit
rendering, WayVNC pixels, touch, sustained navigation, RSS, temperature and
crash/restart recovery remain
physical qualification steps after the pinned runtime closure is eligible.
No release or deployment decision follows from this host optimization alone.

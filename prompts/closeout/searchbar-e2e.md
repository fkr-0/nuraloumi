@projmgrauth
Set repo: nuraloumi

# NuraLoumi Searchbar E2E

Canonical OCP task: NURALOUMI-SEARCHBAR-E2E-20261002
Priority: 95
Lane: shell-search

Prefer and adopt the existing ready runtime packet "NuraLoumi live top-bar search field" rather than creating duplicate implementation scope.

Mission: implement a permanent search affordance between Network and Audio in the live top panel.

Requirements:
- visible search field/control between Network and Audio;
- pointer/touch/keyboard activation goes through one semantic path;
- activation opens/focuses launcher search;
- typed text updates canonical core query and visible launcher results;
- Backspace is Unicode-safe;
- Escape/back clears query before closing according to shell semantics;
- empty query preserves stable selection;
- field truncates/ellipsizes safely at SL101 width;
- no direct provider/system command coupling;
- live Wayland panel rendering and fixture/headless tests both work.

Respect all current active/awaiting-review shell claims. If claimed paths overlap, do not fork implementation; attach to the existing search packet or defer writes until the owning packet clears.

Acceptance: tests for pointer/touch/keyboard, query binding, clear/close behavior and live panel rendering; fmt/clippy/test green; independent review.

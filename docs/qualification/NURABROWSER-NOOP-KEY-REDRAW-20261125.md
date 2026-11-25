# NuraBrowser no-op keypress redraw gate

Date: 2026-11-25. Scope: native chrome keyboard-to-Cairo scheduling.

## Finding and fix

The browser previously set `redraw = true` for every pressed key, including
unhandled key codes and repeated Backspace when the address editor was closed.
This rebuilt the menu, laid out the scene, allocated Cairo pixels and attempted
a full Wayland SHM present despite unchanged visible state. Under key repeat,
the redundant work could be repeated at keyboard event frequency.

The key event branch now compares the bounded `BrowserUi` model and
`MenuState` before/after key handling. It schedules a frame only when either
changes. An accepted menu action independently schedules its frame in the
existing action-dispatch branch. Escape/quit still exit immediately. Address
editing and keyboard menu selection continue to repaint on their transitions.

A focused test exercises 64 Backspace no-ops with zero scheduled model redraws,
then verifies that address focus, text editing and menu selection do request
redraws. Host binary, browser model, layout, supervisor and lint checks are
the regression gate.

This does not establish native ARMv7 time savings, WayVNC pixels or physical
keyboard/touch behavior; those require supervised device qualification. No
device or ABC fleet mutation.

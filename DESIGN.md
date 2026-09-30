# NuraLoumi visual and interaction design

## Character

NuraLoumi should feel like a compact instrument rather than a phone skin:
- deep neutral surfaces;
- violet accent;
- thin structured borders;
- large calm touch targets;
- strong typography hierarchy;
- limited motion;
- almost no decorative chrome.

Reference dark palette:
- base #111218
- raised #181921
- overlay #1c1d27
- card #1f202b
- selected #3a2d5c
- primary text #f4f4f8
- secondary text #bebfcc
- hint #8e90a0
- accent #a78bfa
- warning #fbbf24
- error #f87171

## Geometry

- panel height: 48 logical px default.
- primary menu row: 52 px; compact row: 40 px.
- menu width: 420-520 px depending on output width.
- outer padding: 16 px.
- card/menu radius: 10-12 px.
- border: 1 px.
- shadow: one cheap offset/soft shadow only; disable on constrained mode.
- separator: 1 px with low opacity.

## Primary surfaces

### Top panel
```text
┌──────────────────────────────────────────────────────┐
│ ◈ Apps   ◉ Net   ♪ Audio              73%    23:42 │
└──────────────────────────────────────────────────────┘
```

### Launcher sheet
```text
┌────────────────────────────────────────┐
│ Apps                                   │
│ ┌────────────────────────────────────┐ │
│ │ Search applications…               │ │
│ └────────────────────────────────────┘ │
├────────────────────────────────────────┤
│ Terminal                           ›   │
│ Files                                  │
│ Browser                                │
│ Music                                  │
├────────────────────────────────────────┤
│ Recent                                 │
│ Project / file                         │
└────────────────────────────────────────┘
```

### System sheet
```text
┌────────────────────────────────────────┐
│ System                                 │
├────────────────────────────────────────┤
│ Wi-Fi                     Connected    │
│ Bluetooth                       On     │
│ Brightness                ━━━━━○       │
│ Volume                    ━━━━━━━○     │
├────────────────────────────────────────┤
│ Suspend                                │
│ Restart…                               │
│ Power off…                             │
└────────────────────────────────────────┘
```

## Interaction contract

- Pointer/touch click activates the same typed action as Enter.
- Up/Down skips non-actionable rows and wraps.
- Left returns from submenu; Right enters submenu.
- Escape closes one navigation level, then the surface.
- Search input owns text keys only while focused.
- Destructive actions always enter a confirmation model; renderer cannot bypass it.
- Touch press has pressed feedback; activation occurs on release inside the same hit region.
- Long press is reserved; do not overload it in 0.1.
- Slider keyboard must remain fully usable without touch.

## Motion

- popup enter: 120-180 ms, opacity + <= 8 px translation.
- selection: color only by default; optional 120 ms micro-transition.
- reduced-motion: no translation/scale, only immediate visibility/opacity.
- no idle animation.

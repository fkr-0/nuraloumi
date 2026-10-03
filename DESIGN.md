# NuraLoumi visual and interaction design

## Character

NuraLoumi should feel like a compact instrument rather than a phone skin:
- deep neutral surfaces;
- muted steel accent;
- thin structured borders;
- square, flush geometry;
- large calm touch targets;
- strong typography hierarchy;
- limited motion;
- almost no decorative chrome.

Reference dark palette:
- base #0b0d0f
- raised #101215
- overlay #14171b
- card #171a1f
- selected #242930
- primary text #e8eaed
- secondary text #a9aeb6
- hint #7b818a
- accent #959eac
- warning #c9a25d
- error #d77878

## Geometry

- panel height: 40 logical px default.
- primary menu row: at least 48 logical px; compact structural rows may be smaller.
- menu width: 420-520 px depending on output width.
- outer padding: 16 px.
- card/menu/search/status radius: 0 px.
- border: 1 px.
- shadows: none in the reference software-rendered path.
- selected actionable row: darker fill plus a narrow muted-steel leading rail.
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
│ Launcher                               │
│ Search applications and windows…       │
├────────────────────────────────────────┤
│ Favorites                              │
│ Firefox                                │
│ Terminal                               │
├────────────────────────────────────────┤
│ Open windows                           │
│ Firefox — project docs                 │
│ Terminal — build                       │
├────────────────────────────────────────┤
│ Applications                           │
│ Files                                  │
│ Music                                  │
├────────────────────────────────────────┤
│ More                                   │
│ All applications · Window controls     │
│ Tasks · Desktops · Settings · Power    │
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
- Pointer/touch press outside a transient menu dismisses it by default; the persistent panel stays visible.
- Successful app launch, window focus, or desktop switch dismisses a transient menu by default; continuous controls stay open.
- Long press is reserved; do not overload it in 0.1.
- Slider keyboard must remain fully usable without touch.

## Motion

- popup enter: 120-180 ms, opacity + <= 8 px translation.
- selection: color only by default; optional 120 ms micro-transition.
- reduced-motion: no translation/scale, only immediate visibility/opacity.
- no idle animation.

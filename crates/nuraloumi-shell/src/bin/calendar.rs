use nuraloumi_core::{MenuAction, MenuItem, MenuModel, MenuState, DARK_THEME, LIGHT_THEME};
use nuraloumi_providers::calendar::{month_grid, CalendarDate, CalendarEvent, CalendarProvider};
use nuraloumi_providers::common::Provider;
use nuraloumi_render_cairo::{CairoRenderer, RenderOptions, Scene, Viewport};
use nuraloumi_shell::{
    dismiss_backdrop_margins, load_config_or_default, render_menu_with_glyphs, Theme as ShellTheme,
};
use nuraloumi_wayland::{
    BackendError, DismissBackdropConfig, Frame, Key, MenuConfig, PixelFormat, PlatformEvent,
    SurfaceId, WaylandBackend,
};
use std::process::ExitCode;

fn fixture_events() -> Vec<CalendarEvent> {
    vec![CalendarEvent {
        date: nuraloumi_providers::calendar::date_from_days(
            nuraloumi_providers::calendar::days_from_date(2026, 11, 3),
        ),
        time: Some("14:30".into()),
        summary: "Sample appointment".into(),
        location: Some("Office".into()),
    }]
}
fn shift_month(year: u16, month: u8, delta: i32) -> (u16, u8) {
    let count = i32::from(year) * 12 + i32::from(month) - 1 + delta;
    (
        count.div_euclid(12).clamp(1, i32::from(u16::MAX)) as u16,
        (count.rem_euclid(12) + 1) as u8,
    )
}
fn model(
    year: u16,
    month: u8,
    selected: CalendarDate,
    today: CalendarDate,
    events: &[CalendarEvent],
) -> MenuModel {
    let names = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let mut items = vec![
        MenuItem::action(
            "prev",
            format!("‹  {} {}  ›", names[usize::from(month - 1)], year),
            MenuAction::Custom {
                kind: "month".into(),
                payload: "-1".into(),
            },
        )
        .with_subtitle("PageUp / PageDown change month"),
        MenuItem::status("weekday", "Mo     Tu     We     Th     Fr     Sa     Su"),
    ];
    let grid = month_grid(year, month, events);
    for week in 0..6 {
        let chunk = &grid[week * 7..week * 7 + 7];
        let row = chunk
            .iter()
            .map(|cell| {
                let dot = if cell.has_events { "•" } else { " " };
                let marker = if cell.date == selected { "[" } else { " " };
                let tail = if cell.date == selected { "]" } else { dot };
                format!("{marker}{:02}{tail}", cell.date.day)
            })
            .collect::<Vec<_>>()
            .join(" ");
        items.push(MenuItem::status(format!("week-{week}"), row));
    }
    items.push(MenuItem::separator("events-sep"));
    items.push(MenuItem::status(
        "day-heading",
        format!(
            "Events for {:04}-{:02}-{:02}{}",
            selected.year,
            selected.month,
            selected.day,
            if selected == today { " (today)" } else { "" }
        ),
    ));
    let selected_events: Vec<_> = events.iter().filter(|e| e.date == selected).collect();
    if selected_events.is_empty() {
        items.push(MenuItem::status("no-events", "No events"));
    }
    for (i, event) in selected_events.iter().enumerate().take(20) {
        items.push(
            MenuItem::status(
                format!("event-{i}"),
                format!(
                    "{} {}",
                    event.time.as_deref().unwrap_or("All day"),
                    event.summary
                ),
            )
            .with_subtitle(event.location.clone().unwrap_or_default()),
        );
    }
    MenuModel::new("calendar", "Calendar", items)
}
fn move_days(date: CalendarDate, days: i64) -> CalendarDate {
    nuraloumi_providers::calendar::date_from_days(
        nuraloumi_providers::calendar::days_from_date(date.year, date.month, date.day) + days,
    )
}
fn main() -> ExitCode {
    let program = std::env::args().next().unwrap_or_default();
    let mut args = std::env::args().skip(1);
    let mut fixture = false;
    let mut weather_fixture = program.ends_with("nuraloumi-weather");
    let mut config_path: Option<std::path::PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("nuraloumi-calendar [--fixture] [--live] [--config PATH] [--weather-fixture] (Wayland required except --help)");
                return ExitCode::SUCCESS;
            }
            "--fixture" => fixture = true,
            "--live" => fixture = false,
            "--weather-fixture" => weather_fixture = true,
            "--config" => {
                let Some(path) = args.next() else {
                    eprintln!("nuraloumi-calendar: --config requires a path");
                    return ExitCode::from(2);
                };
                config_path = Some(path.into());
            }
            _ => {
                eprintln!("nuraloumi-calendar: unknown argument: {arg}");
                return ExitCode::from(2);
            }
        }
    }
    if weather_fixture {
        return run_weather_fixture();
    }
    let config = match load_config_or_default(config_path.as_deref()) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("nuraloumi-calendar config: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut provider = CalendarProvider::system();
    let snapshot = if fixture {
        None
    } else {
        match provider.snapshot() {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("calendar: {e}");
                return ExitCode::FAILURE;
            }
        }
    };
    let today = if fixture {
        move_days(
            nuraloumi_providers::calendar::date_from_days(
                nuraloumi_providers::calendar::days_from_date(2026, 11, 3),
            ),
            0,
        )
    } else {
        snapshot.as_ref().expect("live snapshot").today
    };
    let mut selected = today;
    let (mut year, mut month) = (today.year, today.month);
    let mut events = if fixture {
        fixture_events()
    } else {
        snapshot.expect("live snapshot").events
    };
    let mut menu: MenuModel;

    let mut backend = match WaylandBackend::connect() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("calendar Wayland: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = backend.blocking_dispatch() {
        eprintln!("calendar dispatch: {e}");
        return ExitCode::FAILURE;
    }
    let output = backend.outputs().into_iter().next();
    let (mt, mr, mb, ml) = dismiss_backdrop_margins(config.panel_edge, config.panel_height);
    let dismiss = backend
        .create_dismiss_backdrop(DismissBackdropConfig {
            margin_top: mt,
            margin_right: mr,
            margin_bottom: mb,
            margin_left: ml,
            output: output.as_ref().map(|o| o.id),
            namespace: "nuraloumi-calendar-dismiss".into(),
        })
        .ok();
    let surface = match backend.create_menu(MenuConfig {
        width: 520,
        height: 720,
        margin_top: config.panel_height as i32,
        margin_left: 0,
        output: output.as_ref().map(|o| o.id),
        namespace: "nuraloumi-calendar".into(),
    }) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("calendar surface: {e}");
            return ExitCode::FAILURE;
        }
    };
    let renderer = CairoRenderer::default();
    let tokens = match config.theme {
        ShellTheme::Dark => &DARK_THEME,
        ShellTheme::Light => &LIGHT_THEME,
    };
    let mut geometry = None;
    let mut scene: Option<Scene> = None;
    loop {
        if let Err(e) = backend.blocking_dispatch() {
            eprintln!("calendar dispatch: {e}");
            return ExitCode::FAILURE;
        }
        let incoming: Vec<_> = backend.drain_events().collect();
        let mut redraw = false;
        for event in incoming {
            if let Some(ds) = dismiss.filter(|ds| event.surface == Some(*ds)) {
                match event.event {
                    PlatformEvent::Configure {
                        width,
                        height,
                        scale,
                    } => present_transparent(&mut backend, ds, width, height, scale),
                    PlatformEvent::PointerButton { pressed: true, .. }
                    | PlatformEvent::TouchDown { .. }
                    | PlatformEvent::Close => {
                        close_surfaces(&mut backend, surface, dismiss);
                        return ExitCode::SUCCESS;
                    }
                    _ => {}
                }
                continue;
            }
            if event.surface != Some(surface) {
                continue;
            }
            match event.event {
                PlatformEvent::Configure {
                    width,
                    height,
                    scale,
                } => {
                    geometry = Some((width, height, scale));
                    redraw = true;
                }
                PlatformEvent::Close => {
                    close_surfaces(&mut backend, surface, dismiss);
                    return ExitCode::SUCCESS;
                }
                PlatformEvent::Key { key, pressed: true } => {
                    match key {
                        Key::Escape => {
                            close_surfaces(&mut backend, surface, dismiss);
                            return ExitCode::SUCCESS;
                        }
                        Key::Left => selected = move_days(selected, -1),
                        Key::Right => selected = move_days(selected, 1),
                        Key::Up => selected = move_days(selected, -7),
                        Key::Down => selected = move_days(selected, 7),
                        Key::Text(ref t) if t == "t" => selected = today,
                        Key::Text(ref t) if t == "q" => {
                            close_surfaces(&mut backend, surface, dismiss);
                            return ExitCode::SUCCESS;
                        }
                        Key::Text(ref t) if t == "r" => {
                            if !fixture {
                                if let Ok(s) = provider.snapshot() {
                                    events = s.events;
                                }
                            }
                        }
                        Key::Raw(104) => {
                            (year, month) = shift_month(year, month, -1);
                            selected = nuraloumi_providers::calendar::date_from_days(
                                nuraloumi_providers::calendar::days_from_date(year, month, 1),
                            );
                        }
                        Key::Raw(109) => {
                            (year, month) = shift_month(year, month, 1);
                            selected = nuraloumi_providers::calendar::date_from_days(
                                nuraloumi_providers::calendar::days_from_date(year, month, 1),
                            );
                        }
                        Key::Raw(63) => {
                            if !fixture {
                                if let Ok(s) = provider.snapshot() {
                                    events = s.events;
                                }
                            }
                        }
                        Key::Text(ref t) if t == "[" => {
                            (year, month) = shift_month(year, month, -1);
                            selected = nuraloumi_providers::calendar::date_from_days(
                                nuraloumi_providers::calendar::days_from_date(year, month, 1),
                            );
                        }
                        Key::Text(ref t) if t == "]" => {
                            (year, month) = shift_month(year, month, 1);
                            selected = nuraloumi_providers::calendar::date_from_days(
                                nuraloumi_providers::calendar::days_from_date(year, month, 1),
                            );
                        }
                        _ => {}
                    }
                    (year, month) = (selected.year, selected.month);
                    redraw = true;
                }
                PlatformEvent::PointerButton {
                    x,
                    y,
                    pressed: true,
                    ..
                }
                | PlatformEvent::TouchDown { x, y, .. } => {
                    if let Some(hit) = scene
                        .as_ref()
                        .and_then(|sc| sc.hits.iter().find(|hit| hit.rect.contains(x, y)))
                    {
                        if hit.item_id == "prev" {
                            let delta = if x < hit.rect.x + hit.rect.width / 2.0 {
                                -1
                            } else {
                                1
                            };
                            (year, month) = shift_month(year, month, delta);
                            selected = nuraloumi_providers::calendar::date_from_days(
                                nuraloumi_providers::calendar::days_from_date(year, month, 1),
                            );
                            redraw = true;
                        } else if let Some(week) = hit
                            .item_id
                            .strip_prefix("week-")
                            .and_then(|n| n.parse::<usize>().ok())
                        {
                            let fraction = ((x - hit.rect.x) / hit.rect.width).clamp(0.0, 0.999);
                            let col = (fraction * 7.0) as usize;
                            if let Some(cell) = month_grid(year, month, &events).get(week * 7 + col)
                            {
                                selected = cell.date;
                                (year, month) = (selected.year, selected.month);
                                redraw = true;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if redraw {
            menu = model(year, month, selected, today, &events);
            let state = MenuState::new(&menu);
            if let Some((w, h, s)) = geometry {
                let result = render_menu_with_glyphs(
                    &renderer,
                    &menu,
                    &state,
                    Viewport::new(f64::from(w), f64::from(h), f64::from(s.max(1))),
                    tokens,
                    RenderOptions::default(),
                    None,
                    None,
                );
                if let Ok((new_scene, mut buffer)) = result {
                    scene = Some(new_scene);
                    let info = buffer.info();
                    let frame = buffer.with_argb32_bytes(|pixels, _| {
                        backend.present(
                            surface,
                            Frame {
                                width: info.width as u32,
                                height: info.height as u32,
                                stride: info.stride as usize,
                                format: PixelFormat::Argb8888,
                                pixels,
                            },
                        )
                    });
                    if let Ok(Err(e)) = frame {
                        if !matches!(e, BackendError::WouldBlock) {
                            eprintln!("calendar present: {e}");
                        }
                    }
                }
            }
        }
    }
}

fn present_transparent(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    width: u32,
    height: u32,
    scale: i32,
) {
    let scale = scale.max(1) as u32;
    let Some(pw) = width.checked_mul(scale) else {
        return;
    };
    let Some(ph) = height.checked_mul(scale) else {
        return;
    };
    if let Some(len) = (pw as usize)
        .checked_mul(ph as usize)
        .and_then(|n| n.checked_mul(4))
    {
        let pixels = vec![0_u8; len];
        let _ = backend.present(
            surface,
            Frame::packed(pw, ph, PixelFormat::Argb8888, &pixels),
        );
    }
}

fn close_surfaces(backend: &mut WaylandBackend, surface: SurfaceId, dismiss: Option<SurfaceId>) {
    if let Some(ds) = dismiss {
        let _ = backend.destroy_surface(ds);
    }
    let _ = backend.destroy_surface(surface);
}

fn run_weather_fixture() -> ExitCode {
    use nuraloumi_providers::weather::parse_wttr;
    let config = load_config_or_default(None).unwrap_or_default();
    let tokens = match config.theme {
        ShellTheme::Dark => &DARK_THEME,
        ShellTheme::Light => &LIGHT_THEME,
    };
    let sample = r#"{"current_condition":[{"temp_C":"14","humidity":"65","windspeedKmph":"9","weatherCode":"116","weatherDesc":[{"value":"Partly cloudy"}]}],"weather":[{"maxtempC":"17","mintempC":"9","hourly":[{"weatherCode":"113","weatherDesc":[{"value":"Sunny"}]}]},{"maxtempC":"15","mintempC":"8","hourly":[{"weatherCode":"119","weatherDesc":[{"value":"Cloudy"}]}]},{"maxtempC":"12","mintempC":"7","hourly":[{"weatherCode":"296","weatherDesc":[{"value":"Rain"}]}]}],"nearest_area":[{"areaName":[{"value":"Fixture City"}]}]}"#;
    let weather = match parse_wttr(sample, 0) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("weather fixture parse: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut items = vec![
        MenuItem::status(
            "weather-current",
            format!(
                "{}  {}°C  {}",
                weather.condition_glyph.as_deref().unwrap_or("☁"),
                weather.temperature_c.unwrap_or(0),
                weather.condition.as_deref().unwrap_or("Unknown")
            ),
        ),
        MenuItem::status("weather-location", weather.location.unwrap_or_default()),
        MenuItem::status(
            "weather-details",
            format!(
                "Humidity {}% · wind {} km/h",
                weather.humidity_percent.unwrap_or(0),
                weather.wind_kmh.unwrap_or(0)
            ),
        ),
        MenuItem::separator("forecast-divider"),
    ];
    for (i, day) in weather.forecast.into_iter().enumerate() {
        items.push(
            MenuItem::status(
                format!("forecast-{i}"),
                format!(
                    "{}  {}  {}° / {}°",
                    day.label, day.glyph, day.high_c, day.low_c
                ),
            )
            .with_subtitle(day.condition),
        );
    }
    let menu = MenuModel::new("weather", "Weather", items);
    let state = MenuState::new(&menu);
    let mut backend = match WaylandBackend::connect() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("weather Wayland: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = backend.blocking_dispatch() {
        eprintln!("weather dispatch: {e}");
        return ExitCode::FAILURE;
    }
    let surface = match backend.create_menu(MenuConfig {
        width: 500,
        height: 420,
        margin_top: config.panel_height as i32,
        margin_left: 0,
        output: backend.outputs().first().map(|o| o.id),
        namespace: "nuraloumi-weather".into(),
    }) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("weather surface: {e}");
            return ExitCode::FAILURE;
        }
    };
    let renderer = CairoRenderer::default();
    loop {
        if let Err(e) = backend.blocking_dispatch() {
            eprintln!("weather dispatch: {e}");
            return ExitCode::FAILURE;
        }
        let events: Vec<_> = backend.drain_events().collect();
        for event in events {
            if event.surface != Some(surface) {
                continue;
            }
            match event.event {
                PlatformEvent::Key {
                    key: Key::Escape,
                    pressed: true,
                }
                | PlatformEvent::Close => return ExitCode::SUCCESS,
                PlatformEvent::Key {
                    key: Key::Text(ref text),
                    pressed: true,
                } if text == "q" => return ExitCode::SUCCESS,
                PlatformEvent::Configure {
                    width,
                    height,
                    scale,
                } => {
                    let result = render_menu_with_glyphs(
                        &renderer,
                        &menu,
                        &state,
                        Viewport::new(f64::from(width), f64::from(height), f64::from(scale.max(1))),
                        tokens,
                        RenderOptions::default(),
                        None,
                        None,
                    );
                    if let Ok((_, mut buffer)) = result {
                        let info = buffer.info();
                        let frame = buffer.with_argb32_bytes(|pixels, _| {
                            backend.present(
                                surface,
                                Frame {
                                    width: info.width as u32,
                                    height: info.height as u32,
                                    stride: info.stride as usize,
                                    format: PixelFormat::Argb8888,
                                    pixels,
                                },
                            )
                        });
                        if let Ok(Err(error)) = frame {
                            if !matches!(error, BackendError::WouldBlock) {
                                eprintln!("weather present: {error}");
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

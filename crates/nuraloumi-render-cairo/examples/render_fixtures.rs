use std::{error::Error, path::PathBuf};

use nuraloumi_render_cairo::{
    CairoRenderer, InteractionState, MenuItemView, MenuView, RenderOptions, RowKind, Theme,
    Viewport,
};

fn launcher() -> MenuView {
    MenuView {
        id: "launcher".into(),
        title: "Apps".into(),
        items: vec![
            MenuItemView {
                id: "section-apps".into(),
                label: "Applications".into(),
                subtitle: None,
                shortcut: None,
                glyph: None,
                kind: RowKind::Section,
                enabled: false,
            },
            MenuItemView {
                id: "terminal".into(),
                label: "Terminal".into(),
                subtitle: Some("foot — lightweight terminal".into()),
                shortcut: Some("Enter".into()),
                glyph: Some('T'),
                kind: RowKind::Action,
                enabled: true,
            },
            MenuItemView {
                id: "files".into(),
                label: "Files".into(),
                subtitle: Some("Browse local storage".into()),
                shortcut: None,
                glyph: Some('F'),
                kind: RowKind::Action,
                enabled: true,
            },
            MenuItemView {
                id: "browser".into(),
                label: "Browser".into(),
                subtitle: Some("Open web launcher".into()),
                shortcut: None,
                glyph: Some('B'),
                kind: RowKind::Submenu,
                enabled: true,
            },
            MenuItemView {
                id: "sep-1".into(),
                label: String::new(),
                subtitle: None,
                shortcut: None,
                glyph: None,
                kind: RowKind::Separator,
                enabled: false,
            },
            MenuItemView {
                id: "section-recent".into(),
                label: "Recent".into(),
                subtitle: None,
                shortcut: None,
                glyph: None,
                kind: RowKind::Section,
                enabled: false,
            },
            MenuItemView {
                id: "recent-project".into(),
                label: "NuraLoumi".into(),
                subtitle: Some("Recent project".into()),
                shortcut: None,
                glyph: Some('N'),
                kind: RowKind::Action,
                enabled: true,
            },
        ],
    }
}

fn system_menu() -> MenuView {
    MenuView {
        id: "system".into(),
        title: "System".into(),
        items: vec![
            MenuItemView {
                id: "wifi".into(),
                label: "Wi-Fi".into(),
                subtitle: Some("Connected".into()),
                shortcut: None,
                glyph: None,
                kind: RowKind::Status,
                enabled: true,
            },
            MenuItemView {
                id: "bluetooth".into(),
                label: "Bluetooth".into(),
                subtitle: Some("On".into()),
                shortcut: None,
                glyph: None,
                kind: RowKind::Checkable { checked: true },
                enabled: true,
            },
            MenuItemView {
                id: "sep".into(),
                label: String::new(),
                subtitle: None,
                shortcut: None,
                glyph: None,
                kind: RowKind::Separator,
                enabled: false,
            },
            MenuItemView::action("suspend", "Suspend"),
            MenuItemView {
                id: "restart".into(),
                label: "Restart…".into(),
                subtitle: Some("Confirmation required".into()),
                shortcut: None,
                glyph: None,
                kind: RowKind::Action,
                enabled: true,
            },
            MenuItemView {
                id: "power".into(),
                label: "Power off…".into(),
                subtitle: Some("Confirmation required".into()),
                shortcut: None,
                glyph: None,
                kind: RowKind::Action,
                enabled: true,
            },
        ],
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let out = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/fixtures/render"));
    let renderer = CairoRenderer::default();
    let viewport = Viewport::new(800.0, 600.0, 1.0);
    let theme = Theme::dark();

    let launcher_path = out.join("launcher.png");
    renderer.render_png(
        &launcher(),
        &InteractionState {
            selected_id: Some("terminal".into()),
            ..Default::default()
        },
        viewport,
        &theme,
        RenderOptions::default(),
        &launcher_path,
    )?;

    let system_path = out.join("system.png");
    renderer.render_png(
        &system_menu(),
        &InteractionState {
            selected_id: Some("bluetooth".into()),
            ..Default::default()
        },
        viewport,
        &theme,
        RenderOptions::default(),
        &system_path,
    )?;

    println!("{}", launcher_path.display());
    println!("{}", system_path.display());
    Ok(())
}

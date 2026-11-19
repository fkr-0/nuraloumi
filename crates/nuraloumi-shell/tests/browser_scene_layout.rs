use nuraloumi_core::{MenuAction, MenuState, DARK_THEME};
use nuraloumi_render_cairo::{PaintNode, Rect, Viewport};
use nuraloumi_shell::browser::{
    scene::{action_columns, browser_scene, ACTION_HEIGHT, ADDRESS_HEIGHT, TAB_HEIGHT},
    Bookmark, BrowserRpc, BrowserUi, Tab,
};
use serde_json::{json, Value};

fn scene(ui: &BrowserUi, width: f64, height: f64, offset: f64) -> nuraloumi_render_cairo::Scene {
    let menu = ui.menu();
    browser_scene(
        &menu,
        &MenuState::new(&menu),
        Viewport::new(width, height, 1.0),
        &DARK_THEME,
        offset,
    )
}

fn rect<'a>(scene: &'a nuraloumi_render_cairo::Scene, id: &str) -> &'a Rect {
    &scene
        .hits
        .iter()
        .find(|hit| hit.item_id == id)
        .expect(id)
        .rect
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.intersection(b).is_some()
}

#[test]
fn browser_scene_fixed_tabstrip_address_and_three_columns() {
    let ui = BrowserUi::fixture();
    for width in [720.0, 820.0] {
        let first = scene(&ui, width, 600.0, 0.0);
        let scrolled = scene(&ui, width, 600.0, 500.0);
        assert_eq!(action_columns(width), 3);
        for id in ["browser.tab.0", "browser.tab.1"] {
            assert_eq!(rect(&first, id), rect(&scrolled, id));
            assert_eq!(rect(&first, id).height, TAB_HEIGHT);
            assert!(rect(&first, id).height >= 48.0);
            assert_eq!(rect(&first, id).y, 0.0);
        }
        assert_eq!(
            *rect(&first, "browser.address"),
            Rect::new(0.0, TAB_HEIGHT, width, ADDRESS_HEIGHT)
        );
        let cell = width / 3.0;
        assert_eq!(
            *rect(&first, "browser.go"),
            Rect::new(0.0, TAB_HEIGHT + ADDRESS_HEIGHT, cell, ACTION_HEIGHT)
        );
        assert_eq!(rect(&first, "browser.back").x, cell);
        assert_eq!(rect(&first, "browser.forward").x, cell * 2.0);
        for tab in first.hits.iter().filter(|hit| {
            hit.item_id.starts_with("browser.tab.") && hit.item_id[12..].parse::<usize>().is_ok()
        }) {
            for control in first
                .hits
                .iter()
                .filter(|hit| !hit.item_id.starts_with("browser.tab."))
            {
                assert!(
                    !overlaps(tab.rect, control.rect),
                    "{} vs {}",
                    tab.item_id,
                    control.item_id
                );
            }
        }
        assert!(
            !first
                .hits
                .iter()
                .find(|hit| hit.item_id == "browser.tab.0")
                .unwrap()
                .enabled
        );
        assert_eq!(first.hit_test(width / 4.0, TAB_HEIGHT / 2.0), None);
        assert_eq!(
            first.hit_test(width * 0.75, TAB_HEIGHT / 2.0),
            Some("browser.tab.1")
        );
    }
}

#[test]
fn browser_scene_six_tabs_fit_and_controls_change_columns_deterministically() {
    let mut ui = BrowserUi::fixture();
    ui.tabs.tabs = (0..6)
        .map(|n| Tab {
            uri: format!("https://example.invalid/tab-{n}"),
            title: format!("A very long browser title {n} repeated repeated repeated"),
            loading: false,
        })
        .collect();
    ui.tabs.active = 3;
    for (width, columns) in [(480.0, 2), (720.0, 3), (820.0, 3), (1024.0, 4)] {
        let result = scene(&ui, width, 720.0, 0.0);
        assert_eq!(action_columns(width), columns);
        for n in 0..6 {
            let id = format!("browser.tab.{n}");
            let tab = rect(&result, &id);
            assert_eq!(tab.width, width / 6.0);
            assert_eq!(tab.height, TAB_HEIGHT);
            assert!(tab.right() <= width + f64::EPSILON);
            for other in 0..n {
                assert!(!overlaps(
                    *tab,
                    *rect(&result, &format!("browser.tab.{other}"))
                ));
            }
            let hit = result.hit_test(tab.x + tab.width / 2.0, tab.height / 2.0);
            assert_eq!(hit, if n == 3 { None } else { Some(id.as_str()) });
        }
        assert_eq!(
            result
                .hits
                .iter()
                .filter(|hit| hit.item_id.starts_with("browser.tab.")
                    && hit.item_id[12..].parse::<usize>().is_ok())
                .count(),
            6
        );
    }
}

#[test]
fn browser_scene_ellipsizes_labels_without_modifying_navigation_targets() {
    let mut ui = BrowserUi::fixture();
    let target = format!("https://example.invalid/{}", "long-segment/".repeat(16));
    ui.tabs.tabs[1].title = "Very long unicode title ★ café ".repeat(12);
    ui.tabs.tabs[1].uri = target.clone();
    ui.bookmarks.entries = vec![Bookmark {
        url: target.clone(),
        title: "Long".into(),
    }];
    let menu = ui.menu();
    assert!(matches!(menu.item_by_id("browser.tab.1").unwrap().action,
        Some(MenuAction::Custom { ref payload, .. }) if payload == "1"));
    assert!(
        matches!(menu.item_by_id("browser.bookmark.0").unwrap().action,
        Some(MenuAction::Custom { ref payload, .. }) if payload == &target)
    );
    let result = scene(&ui, 480.0, 500.0, 0.0);
    assert!(result
        .paint
        .iter()
        .any(|node| matches!(node, PaintNode::Text { text, .. } if text.ends_with('…'))));
    assert_eq!(ui.tabs.tabs[1].uri, target);
}

#[test]
fn browser_scene_bookmark_scroll_never_moves_fixed_chrome() {
    let mut ui = BrowserUi::fixture();
    ui.bookmarks.entries = (0..8)
        .map(|n| Bookmark {
            url: format!("https://example.invalid/{n}"),
            title: format!("Bookmark {n}"),
        })
        .collect();
    ui.bookmarks.total = 8;
    let menu = ui.menu();
    let top = browser_scene(
        &menu,
        &MenuState::new(&menu),
        Viewport::new(720.0, 312.0, 1.0),
        &DARK_THEME,
        0.0,
    );
    let bottom = browser_scene(
        &menu,
        &MenuState::new(&menu),
        Viewport::new(720.0, 312.0, 1.0),
        &DARK_THEME,
        9999.0,
    );
    assert!(top.scroll.clipped);
    assert_eq!(
        top.scroll.viewport.y,
        TAB_HEIGHT + ADDRESS_HEIGHT + 3.0 * ACTION_HEIGHT
    );
    assert!(bottom.scroll.offset > 0.0);
    assert_eq!(
        rect(&top, "browser.address"),
        rect(&bottom, "browser.address")
    );
    assert_eq!(rect(&top, "browser.tab.1"), rect(&bottom, "browser.tab.1"));
    assert!(bottom
        .hits
        .iter()
        .any(|hit| hit.item_id == "browser.bookmark.7"));
    assert!(bottom
        .hits
        .iter()
        .filter(|hit| hit.item_id.starts_with("browser.bookmark.")
            && hit.item_id != "browser.bookmark.expand"
            && hit.item_id != "browser.bookmark.toggle")
        .all(|hit| hit.rect.y >= bottom.scroll.viewport.y));
}

struct StatusRpc;
impl BrowserRpc for StatusRpc {
    fn request(&mut self, cmd: &str, argument: Option<&str>) -> Result<Value, String> {
        match cmd {
            "uri" => Ok(json!({"uri":"https://example.invalid/new","title":"New"})),
            "tabs" => Ok(
                json!({"tabs":[{"uri":"https://example.invalid/new","title":"New"}],
                "active":0}),
            ),
            "bookmarks" => Ok(json!({"entries":[],"offset":
                argument.unwrap_or("0").parse::<usize>().unwrap() * 8,"total":0})),
            _ => Err("unexpected command".into()),
        }
    }
}

#[test]
fn browser_scene_refresh_keeps_partially_typed_address_and_full_width_hit() {
    let mut ui = BrowserUi::fixture();
    ui.focus_address();
    ui.address.clear();
    ui.text_input("unfinished.example");
    ui.refresh(&mut StatusRpc).unwrap();
    assert_eq!(ui.address, "unfinished.example");
    assert_eq!(ui.page.uri, "https://example.invalid/new");
    let result = scene(&ui, 820.0, 400.0, 0.0);
    assert_eq!(rect(&result, "browser.address").width, 820.0);
    assert_eq!(
        result.hit_test(819.0, TAB_HEIGHT + 12.0),
        Some("browser.address")
    );
}

//! Stable filtering and quick-select helpers.

use crate::model::MenuItem;

/// Default quick-select alphabet, ordered for left-hand-first keyboard access.
pub const QUICK_SELECT_ALPHABET: &str = "asdfhjklqwertyuiopzxcvbnm1234567890";

/// One deterministic quick-select assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickSelectLabel {
    pub item_id: String,
    pub label: String,
}

/// Unicode-safe practical case-insensitive matching over label and subtitle.
///
/// Rust lowercase expansion operates on Unicode scalar values and never slices
/// user text by byte offsets. Matching intentionally preserves source order.
pub fn matches_query(item: &MenuItem, query: &str) -> bool {
    if !item.visible {
        return false;
    }
    let query = fold(query.trim());
    if query.is_empty() {
        return true;
    }

    let mut haystack = fold(&item.label);
    if let Some(subtitle) = &item.subtitle {
        haystack.push(' ');
        haystack.push_str(&fold(subtitle));
    }

    query
        .split_whitespace()
        .all(|needle| haystack.contains(needle))
}

/// Return visible rows in source order. During a non-empty search only matching
/// actionable rows are returned; informational rows do not pollute results.
pub fn filter_visible_rows<'a>(items: &'a [MenuItem], query: &str) -> Vec<&'a MenuItem> {
    let searching = !query.trim().is_empty();
    items
        .iter()
        .filter(|item| {
            item.visible && (!searching || (item.is_actionable() && matches_query(item, query)))
        })
        .collect()
}

/// Generate unique deterministic labels for actionable visible rows.
///
/// Labels grow to multiple characters once the single-character alphabet is
/// exhausted. Headings, statuses, separators, disabled rows, and hidden rows
/// never receive labels.
pub fn quick_select_labels(items: &[MenuItem], query: &str) -> Vec<QuickSelectLabel> {
    filter_visible_rows(items, query)
        .into_iter()
        .filter(|item| item.is_actionable())
        .enumerate()
        .map(|(index, item)| QuickSelectLabel {
            item_id: item.id.clone(),
            label: encode_label(index, QUICK_SELECT_ALPHABET),
        })
        .collect()
}

fn fold(value: &str) -> String {
    value.chars().flat_map(char::to_lowercase).collect()
}

fn encode_label(index: usize, alphabet: &str) -> String {
    let symbols: Vec<char> = alphabet.chars().collect();
    debug_assert!(!symbols.is_empty());
    let base = symbols.len();

    let mut value = index + 1;
    let mut encoded = Vec::new();
    while value > 0 {
        value -= 1;
        encoded.push(symbols[value % base]);
        value /= base;
    }
    encoded.into_iter().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MenuAction, MenuItem};

    fn action(id: &str, label: &str) -> MenuItem {
        MenuItem::action(id, label, MenuAction::Activate { id: id.into() })
    }

    #[test]
    fn unicode_case_matching_is_safe_and_practical() {
        let item = action("open", "ÖFFNEN Café").with_subtitle("Résumé");
        assert!(matches_query(&item, "öffnen"));
        assert!(matches_query(&item, "CAFÉ"));
        assert!(matches_query(&item, "résumé"));
        assert!(!matches_query(&item, "terminal"));
    }

    #[test]
    fn filtering_preserves_source_order() {
        let items = vec![
            action("a", "Terminal"),
            action("b", "Music"),
            action("c", "Terminal Settings"),
        ];
        let ids: Vec<_> = filter_visible_rows(&items, "terminal")
            .into_iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(ids, vec!["a", "c"]);
    }

    #[test]
    fn quick_select_excludes_non_actionable_rows() {
        let items = vec![
            MenuItem::section("section:apps", "Apps"),
            action("terminal", "Terminal"),
            MenuItem::status("status:net", "Offline"),
            action("files", "Files").disabled(),
            MenuItem::separator("sep:one"),
            action("music", "Music"),
        ];
        let labels = quick_select_labels(&items, "");
        assert_eq!(
            labels,
            vec![
                QuickSelectLabel {
                    item_id: "terminal".into(),
                    label: "a".into(),
                },
                QuickSelectLabel {
                    item_id: "music".into(),
                    label: "s".into(),
                },
            ]
        );
    }

    #[test]
    fn quick_select_labels_expand_without_collisions() {
        let count = QUICK_SELECT_ALPHABET.chars().count() + 2;
        let items: Vec<_> = (0..count)
            .map(|index| action(&format!("id-{index}"), &format!("Item {index}")))
            .collect();
        let labels = quick_select_labels(&items, "");
        let mut unique = std::collections::HashSet::new();
        assert!(labels
            .iter()
            .all(|entry| unique.insert(entry.label.clone())));
        assert_eq!(labels[QUICK_SELECT_ALPHABET.chars().count()].label, "aa");
    }
}

use nuraloumi_core::{validate_menu, MenuModel};

#[test]
fn serde_fixture_round_trips_without_semantic_drift() {
    let source = include_str!("fixtures/menu.json");
    let model: MenuModel = serde_json::from_str(source).expect("fixture parses");
    validate_menu(&model).expect("fixture validates");

    let encoded = serde_json::to_string_pretty(&model).expect("fixture serializes");
    let reparsed: MenuModel = serde_json::from_str(&encoded).expect("serialized model parses");
    assert_eq!(reparsed, model);

    let source_value: serde_json::Value = serde_json::from_str(source).unwrap();
    let encoded_value: serde_json::Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(encoded_value, source_value);
}

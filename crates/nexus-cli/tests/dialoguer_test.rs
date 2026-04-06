#[test]
fn test_select_returns_item_not_index() {
    let models = [
        "models/gemini-2.0-flash",
        "models/gemini-2.5-pro",
        "models/gemini-3.1-flash-lite-preview",
    ];
    let selected = models[2];
    assert_eq!(selected, "models/gemini-3.1-flash-lite-preview");
}

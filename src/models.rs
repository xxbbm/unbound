use std::{collections::BTreeMap, sync::LazyLock};
static NAMES: LazyLock<BTreeMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../assets/model-names.json")).unwrap_or_default()
});
pub fn name(product: &str) -> &str {
    NAMES.get(product).map(String::as_str).unwrap_or(product)
}
pub fn photo_file(product: &str) -> String {
    let safe: String = product
        .replace(',', "_")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    format!("Photos/{safe}.png")
}

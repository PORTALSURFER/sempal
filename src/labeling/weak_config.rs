use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const LABEL_RULES_FILE_NAME: &str = "label_rules.toml";

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LabelRulesToml {
    #[serde(default)]
    pub categories: BTreeMap<String, Vec<String>>,
}

pub fn label_rules_path() -> Option<PathBuf> {
    let dir = crate::app_dirs::app_root_dir().ok()?;
    Some(dir.join(LABEL_RULES_FILE_NAME))
}

pub fn load_label_rules_from_app_dir() -> Option<LabelRulesToml> {
    let path = label_rules_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    toml::from_str::<LabelRulesToml>(&text).ok()
}


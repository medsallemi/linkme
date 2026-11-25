use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub subtitle: String,
    pub caption: String,
    pub category_id: i64,
    pub group_id: Option<i64>,
    pub category_name: String,
    pub group_name: Option<String>,
    pub category_icon: String,
    pub category_color: Option<u8>,
    pub revision: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LinkDraft {
    pub id: Option<i64>,
    pub revision: Option<i64>,
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub caption: String,
    #[serde(default = "inbox_id")]
    pub category_id: i64,
    pub group_id: Option<i64>,
    pub new_category: Option<String>,
    pub new_category_icon: Option<String>,
}

fn inbox_id() -> i64 {
    1
}

impl Default for LinkDraft {
    fn default() -> Self {
        Self {
            id: None,
            revision: None,
            url: String::new(),
            title: String::new(),
            subtitle: String::new(),
            caption: String::new(),
            category_id: 1,
            group_id: None,
            new_category: None,
            new_category_icon: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SaveResult {
    pub link: Link,
    pub created: bool,
}

#[derive(Debug, Serialize)]
pub struct LinkGroup {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct Category {
    pub groups: Vec<LinkGroup>,
    pub id: i64,
    pub name: String,
    pub icon: String,
    pub color: Option<u8>,
    pub pinned: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    pub rank: i64,
    pub id: i64,
}

#[derive(Debug, Serialize)]
pub struct LinkPage {
    pub links: Vec<Link>,
    pub next: Option<Cursor>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureMode {
    #[default]
    Prompt,
    Quick,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaptureSettings {
    pub mode: CaptureMode,
    pub shortcut: String,
    #[serde(default = "default_search_shortcut")]
    pub search_shortcut: String,
    #[serde(default = "default_dim_percent")]
    pub dim_percent: u8,
    #[serde(default = "default_panel_transparency")]
    pub panel_transparency: u8,
    #[serde(default = "default_highlight")]
    pub highlight: u8,
}

pub const MAX_DIM_PERCENT: u8 = 40;
pub const DEFAULT_DIM_PERCENT: u8 = 40;
pub const MAX_PANEL_TRANSPARENCY: u8 = 25;
pub const DEFAULT_PANEL_TRANSPARENCY: u8 = 25;
pub const MAX_CAPTURE_HIGHLIGHT: u8 = 7;

const fn default_dim_percent() -> u8 {
    DEFAULT_DIM_PERCENT
}

const fn default_panel_transparency() -> u8 {
    DEFAULT_PANEL_TRANSPARENCY
}

pub const fn default_highlight() -> u8 {
    7
}

fn default_search_shortcut() -> String {
    "Ctrl+Shift+K".into()
}

impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            mode: CaptureMode::Prompt,
            shortcut: "Ctrl+Shift+L".into(),
            search_shortcut: default_search_shortcut(),
            dim_percent: default_dim_percent(),
            panel_transparency: default_panel_transparency(),
            highlight: default_highlight(),
        }
    }
}

#[cfg(test)]
mod settings_defaults_tests {
    use super::*;
    #[test]
    fn brand_is_default_but_existing_explicit_accents_survive() {
        let missing: CaptureSettings =
            serde_json::from_str(r#"{"mode":"prompt","shortcut":"Ctrl+Shift+L"}"#).unwrap();
        assert_eq!(missing.highlight, 7);
        assert_eq!((missing.dim_percent, missing.panel_transparency), (40, 25));
        let explicit: CaptureSettings =
            serde_json::from_str(r#"{"mode":"prompt","shortcut":"Ctrl+Shift+L","highlight":0}"#)
                .unwrap();
        assert_eq!(explicit.highlight, 0);
    }
}

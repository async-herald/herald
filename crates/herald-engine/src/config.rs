use std::path::Path;

use serde::Deserialize;

pub const CONFIG_FILE: &str = "herald.config.json";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub ignore: Vec<String>,
    pub rules: RulesConfig,
    pub thresholds: Thresholds,
    /// Sources `.herald` à exécuter (chargées depuis la base). Vide = catalogue
    /// compilé dans le binaire. Non sérialisé (injecté côté serveur).
    #[serde(skip)]
    pub rule_sources: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RulesConfig {
    pub disable: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Thresholds {
    pub max_file_lines: usize,
    pub max_line_length: usize,
    // 0 = use the per-language default baked into each metric rule.
    pub max_function_lines: usize,
    pub max_params: usize,
    pub max_complexity: usize,
    pub max_nesting: usize,
    pub max_methods: usize,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            max_file_lines: 500,
            // 0 = auto (seuil par langage, voir `long_line::default_max_line_length`).
            max_line_length: 0,
            max_function_lines: 0,
            max_params: 0,
            max_complexity: 0,
            max_nesting: 0,
            max_methods: 0,
        }
    }
}

impl Config {
    pub fn load(dir: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(dir.join(CONFIG_FILE)) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_else(|e| {
            eprintln!("⚠ {CONFIG_FILE} invalide ({e}) — config par défaut utilisée");
            Self::default()
        })
    }

    pub fn is_rule_enabled(&self, name: &str) -> bool {
        !self.rules.disable.iter().any(|d| d == name)
    }
}

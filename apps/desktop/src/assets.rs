//! Ressources embarquées dans le binaire (logo, icônes, polices).
//!
//! Tout est inclus à la compilation avec `include_bytes!` : l'application
//! fonctionne hors ligne et ne dépend d'aucun fichier à côté de l'exécutable.

use std::borrow::Cow;

use anyhow::Result;
use gpui::{App, AssetSource, SharedString};

/// Source d'assets de l'application, branchée via `Application::with_assets`.
pub struct Assets;

macro_rules! embed {
    ($($path:literal),* $(,)?) => {
        &[$(($path, include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/", $path)))),*]
    };
}

/// Fichiers adressables par chemin logique (`img("brand/angel.png")`, `svg().path(..)`).
static FILES: &[(&str, &[u8])] = embed![
    "brand/angel.png",
    "emblem/body.svg",
    "emblem/glow.svg",
    "emblem/halo.svg",
    "emblem/pupil.svg",
    "emblem/rays.svg",
    "emblem/wing-tl-fill.svg",
    "emblem/wing-tl-line.svg",
    "emblem/wing-tr-fill.svg",
    "emblem/wing-tr-line.svg",
    "emblem/wing-bl-fill.svg",
    "emblem/wing-bl-line.svg",
    "emblem/wing-br-fill.svg",
    "emblem/wing-br-line.svg",
    "icons/arrow-right.svg",
    "icons/async.svg",
    "icons/bell.svg",
    "icons/category-dependencies.svg",
    "icons/category-devops.svg",
    "icons/category-pattern.svg",
    "icons/category-quality.svg",
    "icons/category-security.svg",
    "icons/category-tests.svg",
    "icons/weather-cloud.svg",
    "icons/weather-storm.svg",
    "icons/weather-sun.svg",
    "icons/check.svg",
    "icons/chevron-down.svg",
    "icons/chevron-left.svg",
    "icons/chevron-right.svg",
    "icons/close.svg",
    "icons/discord.svg",
    "icons/export.svg",
    "icons/flag.svg",
    "icons/folder-up.svg",
    "icons/git-branch.svg",
    "icons/github.svg",
    "icons/google.svg",
    "icons/halo.svg",
    "icons/lock.svg",
    "icons/ornament-wing-left.svg",
    "icons/ornament-wing-right.svg",
    "icons/pattern-dry.svg",
    "icons/pattern-inject.svg",
    "icons/pattern-layers.svg",
    "icons/pattern-modules.svg",
    "icons/pattern-other.svg",
    "icons/pattern-single.svg",
    "icons/pattern-state.svg",
    "icons/pattern-unlink.svg",
    "icons/pencil.svg",
    "icons/play.svg",
    "icons/plus.svg",
    "icons/refresh.svg",
    "icons/sparkles.svg",
];

/// Polices de la charte Herald (licence SIL OFL 1.1, voir `assets/fonts/OFL.md`).
static FONTS: &[(&str, &[u8])] = embed![
    "fonts/inter-400.ttf",
    "fonts/inter-500.ttf",
    "fonts/inter-600.ttf",
    "fonts/inter-700.ttf",
    "fonts/space-grotesk-600.ttf",
    "fonts/space-grotesk-700.ttf",
    "fonts/jetbrains-mono-400.ttf",
    "fonts/jetbrains-mono-500.ttf",
];

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(FILES
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(FILES
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::new_static(name))
            .collect())
    }
}

/// Fichier d'une police embarquée (`"fonts/inter-400.ttf"`), pour l'export PDF.
pub fn font(path: &str) -> Option<&'static [u8]> {
    FONTS
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, bytes)| *bytes)
}

/// Enregistre les polices embarquées auprès du système de texte de GPUI.
pub fn register_fonts(cx: &App) -> Result<()> {
    let fonts = FONTS
        .iter()
        .map(|(_, bytes)| Cow::Borrowed(*bytes))
        .collect();
    cx.text_system().add_fonts(fonts)
}

//! Charte graphique Async Herald : couleurs, polices et métriques.
//!
//! Les couleurs reprennent `heraldapp-rs/style/_theme.scss` (or, encre brune,
//! fond crème).

use gpui::{Hsla, rgb};

/// Familles de polices embarquées (voir [`crate::assets`]).
pub mod fonts {
    pub const SANS: &str = "Inter";
    /// Nom de famille enregistré dans les fichiers Fontsource de Space Grotesk.
    /// Réservé au logotype et à la note du projet.
    pub const DISPLAY: &str = "Space Grotesk Light";
    pub const MONO: &str = "JetBrains Mono";
}

/// Tailles, espacements et rayons. Aucune autre valeur en dur dans `src/ui/`.
pub mod metrics {
    use gpui::{Pixels, px};

    pub const TEXT_XS: Pixels = px(11.);
    pub const TEXT_SM: Pixels = px(12.);
    pub const TEXT_BASE: Pixels = px(13.);
    pub const TEXT_MD: Pixels = px(14.);
    pub const TEXT_LG: Pixels = px(16.);
    pub const TEXT_XL: Pixels = px(20.);
    /// Note du projet, seul grand chiffre de l'interface.
    pub const TEXT_SCORE: Pixels = px(40.);

    pub const SPACE_1: Pixels = px(4.);
    pub const SPACE_2: Pixels = px(8.);
    pub const SPACE_3: Pixels = px(12.);
    pub const SPACE_4: Pixels = px(16.);
    pub const SPACE_5: Pixels = px(20.);
    pub const SPACE_6: Pixels = px(24.);
    pub const SPACE_8: Pixels = px(32.);

    pub const RADIUS: Pixels = px(6.);
    pub const RADIUS_LG: Pixels = px(8.);

    pub const CONTROL_H: Pixels = px(28.);
    pub const BAR_H: Pixels = px(4.);
    pub const DOT: Pixels = px(6.);
    pub const ICON: Pixels = px(16.);
    pub const ICON_SM: Pixels = px(14.);
}

/// Jetons de couleur. `Copy` : les composants le reçoivent par valeur.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub bg_page: Hsla,
    pub bg_surface: Hsla,
    pub bg_hover: Hsla,
    pub bg_active: Hsla,
    pub border: Hsla,
    pub border_soft: Hsla,

    pub ink: Hsla,
    pub ink_muted: Hsla,
    pub ink_soft: Hsla,

    /// Or de la marque : fond du bouton principal et barres de progression.
    pub accent: Hsla,
    /// Texte posé sur `accent`.
    pub on_accent: Hsla,
    /// Or lisible en texte sur fond clair (liens, points gagnés).
    pub accent_text: Hsla,

    pub success: Hsla,
    /// Fond d'une ligne marquée comme corrigée.
    pub success_bg: Hsla,
    /// Fond doré très clair (bandeau du plan de correction).
    pub accent_soft: Hsla,
    pub warning: Hsla,
    pub danger: Hsla,
    pub track: Hsla,

    /// Emblème animé du Herald (`herald-pro-animated.svg`).
    pub emblem_wing: Hsla,
    pub emblem_wing_deep: Hsla,
    pub emblem_line: Hsla,

    pub code_bg: Hsla,
    pub code_text: Hsla,
    pub code_comment: Hsla,
    pub code_line_number: Hsla,
    pub code_highlight_bg: Hsla,
    pub code_highlight_fg: Hsla,
    /// Éditeur de code : mots-clés, chaînes, nombres, sélection, ligne courante.
    pub code_keyword: Hsla,
    pub code_string: Hsla,
    pub code_number: Hsla,
    pub code_selection: Hsla,
    pub code_current_line: Hsla,
    /// Fond des lignes qui portent un problème, selon la gravité.
    pub code_line_critical: Hsla,
    pub code_line_warning: Hsla,
    /// Correctif proposé : fond et texte de la ligne corrigée.
    pub code_fix_bg: Hsla,
    pub code_fix_fg: Hsla,
}

fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

impl Theme {
    pub fn light() -> Self {
        Self {
            bg_page: hex(0xF5F0E6),
            bg_surface: hex(0xFFFFFF),
            bg_hover: hex(0xF8F4EA),
            bg_active: hex(0xF1E6C8),
            border: hex(0xE3D9C0),
            border_soft: hex(0xEEE8DA),

            ink: hex(0x2B2318),
            ink_muted: hex(0x6B6357),
            ink_soft: hex(0x8A7D62),

            accent: hex(0xEAA11A),
            on_accent: hex(0x2B2318),
            accent_text: hex(0x7A5A12),

            success: hex(0x2F7A4A),
            success_bg: hex(0xF2F8F3),
            accent_soft: hex(0xFFFBEF),
            warning: hex(0x8F5F00),
            danger: hex(0xB8372A),
            track: hex(0xECE3CD),

            emblem_wing: hex(0xFFFDF7),
            emblem_wing_deep: hex(0xF3E6C4),
            emblem_line: hex(0xB8914A),

            code_bg: hex(0xFBF8F1),
            code_text: hex(0x2B2318),
            code_comment: hex(0x8A7D62),
            code_line_number: hex(0xB0A486),
            code_highlight_bg: hex(0xF8E3DE),
            code_highlight_fg: hex(0x7A1E14),
            code_keyword: hex(0x8A3FA0),
            code_string: hex(0x2F7A4A),
            code_number: hex(0x9A5B0E),
            code_selection: hex(0xF3DFA8),
            code_current_line: hex(0xF5EFE2),
            code_line_critical: hex(0xFBE9E5),
            code_line_warning: hex(0xFBF1D9),
            code_fix_bg: hex(0xE4F1E6),
            code_fix_fg: hex(0x1F5C34),
        }
    }
}

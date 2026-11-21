//! Composants visuels sans état, réutilisés par les écrans.

mod button;
mod code_area;
mod herald_emblem;
mod herald_ornaments;
mod icon;
mod labels;
mod logo;
mod progress_bar;
mod text_input;

pub use button::{Button, ButtonVariant};
pub use code_area::{CodeArea, CodeAreaEvent, Diagnostic};
pub use herald_emblem::HeraldEmblem;
pub use herald_ornaments::{herald_banner, herald_banner_with};
pub use icon::{Icon, IconName};
pub use labels::{GradeText, SeverityLabel, grade_color};
pub use logo::Logo;
pub use progress_bar::ProgressBar;
pub use text_input::{TextInput, TextInputEvent};

/// Raccourcis clavier des champs de saisie, à enregistrer une fois au démarrage.
pub fn bind_input_keys(cx: &mut gpui::App) {
    text_input::bind_text_input_keys(cx);
    code_area::bind_code_area_keys(cx);
}

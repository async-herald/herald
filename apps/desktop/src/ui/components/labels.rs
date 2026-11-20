use gpui::{App, FontWeight, Hsla, IntoElement, ParentElement, RenderOnce, Styled, Window, div};

use crate::model::{Grade, GradeTone, Severity};
use crate::theme::{Theme, metrics};

/// Couleur de texte d'une note selon son ton.
pub fn grade_color(grade: Grade, theme: &Theme) -> Hsla {
    match grade.tone() {
        GradeTone::Good => theme.success,
        GradeTone::Average => theme.warning,
        GradeTone::Poor => theme.danger,
    }
}

/// Note (A+…F) en texte coloré, sans fond. `None` affiche un tiret.
#[derive(IntoElement)]
pub struct GradeText {
    grade: Option<Grade>,
    theme: Theme,
}

impl GradeText {
    pub fn new(grade: Option<Grade>, theme: Theme) -> Self {
        Self { grade, theme }
    }
}

impl RenderOnce for GradeText {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let (label, color) = match self.grade {
            Some(grade) => (grade.label(), grade_color(grade, &self.theme)),
            None => ("–", self.theme.ink_soft),
        };
        div()
            .flex_none()
            .text_size(metrics::TEXT_SM)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(color)
            .child(label)
    }
}

/// Gravité : pastille ronde + mot coloré.
#[derive(IntoElement)]
pub struct SeverityLabel {
    severity: Severity,
    theme: Theme,
}

impl SeverityLabel {
    pub fn new(severity: Severity, theme: Theme) -> Self {
        Self { severity, theme }
    }
}

impl RenderOnce for SeverityLabel {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let color = match self.severity {
            Severity::Critical => self.theme.danger,
            Severity::Warning => self.theme.warning,
            Severity::Info => self.theme.ink_soft,
        };
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(metrics::SPACE_2)
            .text_size(metrics::TEXT_SM)
            .font_weight(FontWeight::MEDIUM)
            .text_color(color)
            .child(div().size(metrics::DOT).rounded_full().bg(color))
            .child(self.severity.label())
    }
}

//! Écran « Problèmes » : trois vues du même rapport.
//!
//! - **Plan** (`plan.rs`) : corrections triées par points, note estimée.
//! - **Revue** (`review.rs`) : le code, avec la fiche sous la ligne fautive.
//! - **Code** (`ide.rs`) : l'éditeur, pour corriger.

use gpui::{AnyElement, Div, FontWeight, Hsla, div, prelude::*, px};

use super::app::{HeraldApp, WorkspaceView};
use super::components::{Button, ButtonVariant, IconName, Logo};
use super::home::{page_title, surface};
use crate::model::{Grade, Severity, Usage};
use crate::theme::{Theme, metrics};

const BADGE_H: gpui::Pixels = px(20.);
const NOTICE_LOGO_SIZE: gpui::Pixels = px(20.);

impl HeraldApp {
    pub(super) fn render_analysis(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let (Some(project), Some(report)) = (self.project(), self.report()) else {
            return div().into_any_element();
        };
        let body = match self.workspace_view {
            WorkspaceView::List => self.render_correction_plan(report, theme, cx),
            WorkspaceView::Review => self.render_review(report, theme, cx),
            // Code et Dépendances : contenu avec ses marges, défilant.
            WorkspaceView::Code => padded(
                "code-view",
                div()
                    .flex()
                    .flex_col()
                    .gap(metrics::SPACE_5)
                    .child(self.workspace_header(theme, cx))
                    .children(self.render_analysis_card(theme, cx))
                    .child(self.render_ide(theme, cx)),
            ),
            WorkspaceView::Dependencies => padded(
                "dependencies-view",
                div()
                    .flex()
                    .flex_col()
                    .gap(metrics::SPACE_5)
                    .child(self.workspace_header(theme, cx))
                    .child(self.render_dependencies(theme, cx)),
            ),
        };
        // Note gagnée grâce aux corrections : bandeau au-dessus de la vue.
        let body = match self.celebration {
            Some(grade) => div()
                .size_full()
                .flex()
                .flex_col()
                .child(
                    div()
                        .px(metrics::SPACE_6)
                        .pt(metrics::SPACE_3)
                        .child(celebration_notice(&project.name, grade, theme, cx)),
                )
                .child(div().flex_1().min_h_0().child(body))
                .into_any_element(),
            None => body,
        };
        // Le panneau de la note reste à droite, quel que soit l'onglet.
        div()
            .size_full()
            .flex()
            .child(div().flex_1().min_w_0().h_full().child(body))
            .child(self.render_score_aside(report, theme, cx))
            .into_any_element()
    }

    /// En-tête des vues Code et Dépendances : projet et bascule des vues (la
    /// relance est dans le panneau de la note).
    fn workspace_header(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(project) = self.project() else {
            return div().into_any_element();
        };
        div()
            .flex()
            .items_end()
            .justify_between()
            .gap(metrics::SPACE_4)
            .child(page_title(
                project.name.clone(),
                project.display_path().to_string(),
                theme,
            ))
            .child(view_switch(self.workspace_view, theme, cx))
            .into_any_element()
    }
}

/// Vue avec les marges de l'écran, qui défile verticalement.
fn padded(id: &'static str, content: impl IntoElement) -> AnyElement {
    div()
        .id(id)
        .size_full()
        .overflow_y_scroll()
        .px(metrics::SPACE_8)
        .py(metrics::SPACE_6)
        .child(content)
        .into_any_element()
}

/// Points rapportés, courts : « +5 », « +0,4 ».
pub(super) fn short_points(points: f32) -> String {
    if points >= 0.95 {
        format!("+{}", points.round() as i64)
    } else {
        format!("+{points:.1}").replace('.', ",")
    }
}

pub(super) fn severity_color(severity: Severity, theme: &Theme) -> Hsla {
    match severity {
        Severity::Critical => theme.danger,
        Severity::Warning => theme.warning,
        Severity::Info => theme.ink_soft,
    }
}

/// Badge de gravité (« Critique ») de la fiche.
pub(super) fn severity_badge(severity: Severity, theme: Theme) -> impl IntoElement {
    let bg = match severity {
        Severity::Critical => theme.code_line_critical,
        Severity::Warning => theme.code_line_warning,
        Severity::Info => theme.track,
    };
    badge(severity.label(), theme)
        .bg(bg)
        .border_color(bg)
        .text_color(severity_color(severity, &theme))
}

pub(super) fn badge(label: &'static str, _theme: Theme) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .h(BADGE_H)
        .px(metrics::SPACE_2)
        .rounded(metrics::RADIUS)
        .border_1()
        .text_size(metrics::TEXT_XS)
        .font_weight(FontWeight::MEDIUM)
        .child(label)
}

/// Petit titre de section en capitales (« CORRECTIF PROPOSÉ »).
pub(super) fn section_caption(text: &str, theme: Theme) -> impl IntoElement {
    div()
        .text_size(metrics::TEXT_XS)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.ink_soft)
        .child(text.to_uppercase())
}

/// « Relancer l'analyse » du projet affiché. Pendant l'analyse : inactif ;
/// quota du jour épuisé : inactif jusqu'au lendemain.
pub(super) fn rerun_button(
    usage: Option<Usage>,
    analyzing: bool,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> AnyElement {
    if analyzing {
        return div()
            .opacity(0.6)
            .child(Button::new("rerun", "Analyse en cours…", theme).icon(IconName::Refresh))
            .into_any_element();
    }
    if usage.is_some_and(|usage| usage.is_exhausted()) {
        return div()
            .opacity(0.6)
            .child(Button::new(
                "rerun",
                "Quota atteint · il revient demain",
                theme,
            ))
            .into_any_element();
    }
    Button::new("rerun", "Relancer l'analyse", theme)
        .icon(IconName::Refresh)
        .on_click(cx.listener(|app, _, _, cx| app.start_analysis(cx)))
        .into_any_element()
}

/// Note gagnée grâce aux corrections : une ligne, sans emphase.
pub(super) fn celebration_notice(
    project: &str,
    grade: Grade,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> impl IntoElement + use<> {
    surface(theme)
        .flex()
        .items_center()
        .gap(metrics::SPACE_3)
        .px(metrics::SPACE_4)
        .py(metrics::SPACE_2)
        .child(Logo::new(NOTICE_LOGO_SIZE))
        .child(div().flex_1().child(format!(
            "Avec vos corrections, {project} passe en {grade} (estimation)."
        )))
        .child(
            Button::new("dismiss-celebration", "Fermer", theme)
                .variant(ButtonVariant::Ghost)
                .on_click(cx.listener(|app, _, _, cx| app.dismiss_celebration(cx))),
        )
}

/// Bascule Plan / Revue / Code / Dépendances de l'écran d'analyse.
pub(super) fn view_switch(
    current: WorkspaceView,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> impl IntoElement {
    let item = |id: &'static str, label: &'static str, view: WorkspaceView| {
        let active = current == view;
        div()
            .id(id)
            .flex()
            .items_center()
            .h_full()
            .px(metrics::SPACE_3)
            .cursor_pointer()
            .when(active, |this| {
                this.bg(theme.bg_active).font_weight(FontWeight::MEDIUM)
            })
            .when(!active, |this| {
                this.text_color(theme.ink_muted)
                    .hover(|style| style.bg(theme.bg_hover))
            })
            .on_click(
                cx.listener(move |app, _, window, cx| app.set_workspace_view(view, window, cx)),
            )
            .child(label)
    };
    div()
        .flex()
        .h(metrics::CONTROL_H)
        .rounded(metrics::RADIUS)
        .overflow_hidden()
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg_surface)
        .child(item("view-list", "Plan", WorkspaceView::List))
        .child(div().w(px(1.)).h_full().bg(theme.border))
        .child(item("view-review", "Revue", WorkspaceView::Review))
        .child(div().w(px(1.)).h_full().bg(theme.border))
        .child(item("view-code", "Code", WorkspaceView::Code))
        .child(div().w(px(1.)).h_full().bg(theme.border))
        .child(item(
            "view-deps",
            "Dépendances",
            WorkspaceView::Dependencies,
        ))
}

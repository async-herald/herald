//! Écran « Problèmes » : quatre vues du même rapport.
//!
//! - **Général** (`general.rs`) : les catégories, en liste, et le détail de l'une d'elles.
//! - **Plan** (`plan.rs`) : corrections triées par points, note estimée.
//! - **Patterns** (`patterns.rs`) : une boîte par pattern de conception, avec des astuces.
//! - **Code** (`ide.rs`) : l'éditeur, pour corriger, ou la carte du code.

use gpui::{AnyElement, FontWeight, Hsla, div, prelude::*, px};

use super::app::{HeraldApp, WorkspaceView};
use super::components::{Button, ButtonVariant, IconName, Logo};
use super::home::{count_label, surface};
use crate::model::{Grade, Severity, Usage};
use crate::theme::{Theme, metrics};

const NOTICE_LOGO_SIZE: gpui::Pixels = px(20.);

impl HeraldApp {
    pub(super) fn render_analysis(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let (Some(project), Some(report)) = (self.project(), self.report()) else {
            return div().into_any_element();
        };
        let body = match self.workspace_view {
            WorkspaceView::General => self.render_general_page(report, theme, cx),
            WorkspaceView::List => self.render_correction_plan(report, theme, cx),
            WorkspaceView::Patterns => padded(
                "patterns-view",
                div()
                    .flex()
                    .flex_col()
                    .gap(metrics::SPACE_5)
                    .child(self.workspace_header(theme, cx))
                    .children(self.render_analysis_card(theme, cx))
                    .child(self.render_patterns(report, theme, cx)),
            ),
            // Code et Dépendances : contenu avec ses marges, défilant.
            WorkspaceView::Code => padded(
                "code-view",
                div()
                    .flex()
                    .flex_col()
                    .gap(metrics::SPACE_5)
                    .child(self.workspace_header(theme, cx))
                    .children(self.render_analysis_card(theme, cx))
                    .child(self.code_switch(theme, cx))
                    .map(|this| {
                        if self.code_map_open {
                            this.child(self.code_map_card(report, theme, cx))
                        } else {
                            this.child(self.render_ide(theme, cx))
                        }
                    }),
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

    /// En-tête du projet : nom et chemin, dernière analyse, puis les onglets
    /// soulignés (la note est dans le panneau de droite).
    pub(super) fn workspace_header(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(project) = self.project() else {
            return div().into_any_element();
        };
        let report = project.report.as_ref();
        let subline = match report {
            Some(report) => {
                let open = report
                    .issues
                    .iter()
                    .enumerate()
                    .filter(|(ix, issue)| !issue.review && !report.is_resolved(*ix))
                    .count() as u32;
                format!(
                    "Analysé {} · {} · {}",
                    report.summary.analyzed_at,
                    count_label(report.summary.file_count, "fichier", "fichiers"),
                    count_label(open, "problème ouvert", "problèmes ouverts")
                )
            }
            None => "Pas encore analysé".to_owned(),
        };

        let title = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(metrics::SPACE_1)
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(metrics::SPACE_3)
                    .child(
                        div()
                            .flex_none()
                            .text_size(metrics::TEXT_XL)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(project.name.clone()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(metrics::TEXT_SM)
                            .text_color(theme.ink_muted)
                            .child(project.display_path().to_string()),
                    ),
            )
            .child(
                div()
                    .text_size(metrics::TEXT_SM)
                    .text_color(theme.ink_muted)
                    .child(subline),
            );

        let top = div()
            .flex()
            .items_center()
            .gap(metrics::SPACE_4)
            .child(title);

        div()
            .flex()
            .flex_col()
            .gap(metrics::SPACE_4)
            .child(top)
            .child(
                div()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(view_switch(self.workspace_view, theme, cx)),
            )
            .into_any_element()
    }

    /// Bascule de l'onglet Code : l'éditeur ou la carte du code.
    fn code_switch(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let option = |id: &'static str, label: &'static str, map: bool| {
            Button::new(id, label, theme)
                .variant(if self.code_map_open == map {
                    ButtonVariant::Default
                } else {
                    ButtonVariant::Ghost
                })
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.code_map_open = map;
                    cx.notify();
                }))
        };
        div()
            .flex()
            .gap(metrics::SPACE_2)
            .child(option("code-editor", "Éditeur", false))
            .child(option("code-map", "Carte du code", true))
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
///
/// `prominent` : action principale du panneau « Votre note » (or plein, grande,
/// toute la largeur) ; sinon bouton secondaire (page du projet, où « Voir les
/// problèmes » est l'action principale).
pub(super) fn rerun_button(
    usage: Option<Usage>,
    analyzing: bool,
    prominent: bool,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> AnyElement {
    let style = |button: Button| {
        if prominent {
            button.variant(ButtonVariant::Primary).large().full_width()
        } else {
            button
        }
    };
    let dimmed = |button: Button| {
        div()
            .opacity(0.6)
            .when(prominent, |this| this.w_full())
            .child(style(button))
            .into_any_element()
    };
    if analyzing {
        return dimmed(Button::new("rerun", "Analyse en cours…", theme).icon(IconName::Refresh));
    }
    if usage.is_some_and(|usage| usage.is_exhausted()) {
        return dimmed(Button::new(
            "rerun",
            "Quota atteint · il revient demain",
            theme,
        ));
    }
    style(Button::new("rerun", "Relancer l'analyse", theme).icon(IconName::Refresh))
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

/// Bascule Plan / Patterns / Code / Dépendances de l'écran d'analyse.
fn view_switch(
    current: WorkspaceView,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> impl IntoElement {
    let item = |id: &'static str, label: &'static str, view: WorkspaceView| {
        let active = current == view;
        div()
            .id(id)
            .py(metrics::SPACE_2)
            .mb(px(-1.))
            .border_b_2()
            .cursor_pointer()
            .border_color(if active {
                theme.ink
            } else {
                gpui::transparent_black()
            })
            .when(active, |this| this.font_weight(FontWeight::SEMIBOLD))
            .when(!active, |this| {
                this.text_color(theme.ink_muted)
                    .hover(|style| style.text_color(theme.ink))
            })
            .on_click(
                cx.listener(move |app, _, window, cx| app.set_workspace_view(view, window, cx)),
            )
            .child(label)
    };
    div()
        .flex()
        .gap(metrics::SPACE_6)
        .child(item("view-general", "Général", WorkspaceView::General))
        .child(item("view-list", "Plan", WorkspaceView::List))
        .child(item("view-patterns", "Patterns", WorkspaceView::Patterns))
        .child(item("view-code", "Code", WorkspaceView::Code))
        .child(item(
            "view-deps",
            "Dépendances",
            WorkspaceView::Dependencies,
        ))
}

//! Écran « Problèmes », vue Plan : les corrections triées par points
//! rapportés, à cocher, avec la note estimée qui monte dans la colonne de droite.

use gpui::{AnyElement, FontWeight, SharedString, div, prelude::*, px, relative};

use super::analysis::{rerun_button, severity_color, short_points, view_switch};
use super::app::{HeraldApp, WorkspaceView};
use super::components::{Button, ButtonVariant, Icon, IconName, grade_color};
use super::home::count_label;
use crate::export::Format;
use crate::model::{AnalysisReport, Grade};
use crate::theme::{Theme, fonts, metrics};

/// Corrections proposées au plus dans « Gains rapides ».
const MAX_QUICK: usize = 5;
const ASIDE_WIDTH: gpui::Pixels = px(320.);
/// Panneau « Votre note » réduit : une bande avec la note.
const COLLAPSED_WIDTH: gpui::Pixels = px(56.);
const COLLAPSED_GRADE: gpui::Pixels = px(26.);
/// Réglage mémorisé : panneau « Votre note » réduit.
pub(super) const SCORE_PANEL_COLLAPSED: &str = "score_panel_collapsed";
const CHECK_SIZE: gpui::Pixels = px(16.);
const GAIN_WIDTH: gpui::Pixels = px(56.);
const BIG_GRADE: gpui::Pixels = px(52.);
const BAR_H: gpui::Pixels = px(10.);

impl HeraldApp {
    /// Ouvre la revue de code sur le groupe `gi`.
    pub(super) fn open_review(&mut self, gi: usize, cx: &mut Context<Self>) {
        self.selected_group = Some(gi);
        self.selected_occurrence = 0;
        self.workspace_view = WorkspaceView::Review;
        cx.notify();
    }

    pub(super) fn render_correction_plan(
        &self,
        report: &AnalysisReport,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(project) = self.project() else {
            return div().into_any_element();
        };
        let (order, quick) = report.correction_plan(MAX_QUICK);
        let current = report.summary.grade();
        let target_score = report.score_with(&order[..quick]);
        let target = Grade::from_score(target_score);

        let title = if order.is_empty() {
            "Aucun problème à corriger".to_owned()
        } else if target.is_better_than(current) {
            format!(
                "Passez de {current} à {target} en {}",
                count_label(quick as u32, "correction", "corrections")
            )
        } else {
            format!(
                "Les {} qui rapportent le plus",
                count_label(quick as u32, "correction", "corrections")
            )
        };

        let rows = |range: &[usize], cx: &mut Context<Self>| {
            range
                .iter()
                .map(|&gi| plan_row(gi, report, theme, cx))
                .collect::<Vec<_>>()
        };
        let quick_rows = rows(&order[..quick], cx);
        let rest_rows = rows(&order[quick..], cx);

        let list = div()
            .flex()
            .flex_col()
            .rounded(metrics::RADIUS_LG)
            .overflow_hidden()
            .bg(theme.bg_surface)
            .border_1()
            .border_color(theme.border)
            .child(caption("Gains rapides · triés par points rapportés", theme))
            .children(quick_rows)
            .when(quick > 0 && target_score > report.summary.score, |this| {
                this.child(
                    div()
                        .px(metrics::SPACE_4)
                        .py(metrics::SPACE_2)
                        .border_t_1()
                        .border_color(theme.border)
                        .bg(theme.accent_soft)
                        .text_color(theme.accent_text)
                        .font_weight(FontWeight::MEDIUM)
                        .child(format!(
                            "↑ {} font passer la note à {target} ({target_score})",
                            if quick == 1 {
                                "Cette correction".to_owned()
                            } else {
                                format!("Ces {quick} corrections")
                            }
                        )),
                )
            })
            .when(!rest_rows.is_empty(), |this| {
                this.child(caption("Ensuite", theme)).children(rest_rows)
            });

        div()
            .size_full()
            .flex()
            .child(
                div()
                    .id("plan-main")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(metrics::SPACE_4)
                            .px(metrics::SPACE_8)
                            .py(metrics::SPACE_6)
                            .child(
                                div()
                                    .flex()
                                    .items_end()
                                    .gap(metrics::SPACE_3)
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(metrics::SPACE_1)
                                            .child(
                                                div()
                                                    .text_size(metrics::TEXT_SM)
                                                    .text_color(theme.ink_muted)
                                                    .child(format!(
                                                        "{} · plan de correction",
                                                        project.name
                                                    )),
                                            )
                                            .child(
                                                div()
                                                    .text_size(TITLE_SIZE)
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .child(title),
                                            ),
                                    )
                                    .child(view_switch(self.workspace_view, theme, cx)),
                            )
                            .children(self.render_analysis_card(theme, cx))
                            .when(!order.is_empty(), |this| this.child(list)),
                    ),
            )
            .into_any_element()
    }

    /// Panneau de la note, à droite de tous les onglets du projet (Plan, Revue,
    /// Code, Dépendances) : note actuelle et estimée, relance de l'analyse.
    pub(super) fn render_score_aside(
        &self,
        report: &AnalysisReport,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(project) = self.project() else {
            return div().into_any_element();
        };
        if self.score_panel_collapsed {
            return collapsed_aside(report, theme, cx);
        }
        let (order, _) = report.correction_plan(MAX_QUICK);
        // Relancer, puis télécharger le rapport (PDF ou Markdown).
        let actions = div()
            .flex()
            .flex_wrap()
            .gap(metrics::SPACE_2)
            .child(rerun_button(
                self.usage(),
                self.is_analyzing(&project.path),
                theme,
                cx,
            ))
            .child(
                Button::new("export-pdf", "Rapport PDF", theme)
                    .icon(IconName::Export)
                    .on_click(cx.listener(|app, _, _, cx| app.export_report(Format::Pdf, cx))),
            )
            .child(
                Button::new("export-markdown", "Markdown", theme)
                    .variant(ButtonVariant::Ghost)
                    .on_click(cx.listener(|app, _, _, cx| app.export_report(Format::Markdown, cx))),
            )
            .into_any_element();
        // Bouton « Réduire » en haut à droite du panneau.
        div()
            .relative()
            .h_full()
            .flex_none()
            .child(score_aside(report, &order, actions, theme))
            .child(
                div()
                    .absolute()
                    .top(metrics::SPACE_5)
                    .right(metrics::SPACE_4)
                    .child(panel_toggle(
                        "score-panel-collapse",
                        IconName::ChevronRight,
                        theme,
                        cx,
                    )),
            )
            .into_any_element()
    }

    /// Réduit ou rouvre le panneau « Votre note » ; le choix est gardé.
    fn toggle_score_panel(&mut self, cx: &mut Context<Self>) {
        self.score_panel_collapsed = !self.score_panel_collapsed;
        crate::store::set_setting(SCORE_PANEL_COLLAPSED, self.score_panel_collapsed);
        cx.notify();
    }

    /// Demande où enregistrer le rapport du projet affiché, l'écrit, puis le
    /// montre dans le Finder (l'Explorateur sous Windows).
    pub(super) fn export_report(&mut self, format: Format, cx: &mut Context<Self>) {
        let (Some(project), Some(report)) = (self.project(), self.report()) else {
            return;
        };
        let advisories: Vec<&crate::model::Advisory> = self
            .alerts
            .iter()
            .filter(|alert| alert.project_path == project.path)
            .collect();
        let bytes = match crate::export::render(format, &project.name, report, &advisories) {
            Ok(bytes) => bytes,
            Err(err) => {
                eprintln!("[export] {err}");
                return;
            }
        };
        let name = crate::export::file_name(&project.name, report, format);
        let directory = dirs::download_dir()
            .or_else(crate::paths::home_dir)
            .unwrap_or_default();
        let target = cx.prompt_for_new_path(&directory, Some(&name));
        cx.spawn(async move |_, cx| {
            let Ok(Ok(Some(path))) = target.await else {
                return;
            };
            match std::fs::write(&path, &bytes) {
                Ok(()) => {
                    cx.update(|cx| cx.reveal_path(&path)).ok();
                }
                Err(err) => eprintln!("[export] écriture de {} : {err}", path.display()),
            }
        })
        .detach();
    }
}

const TITLE_SIZE: gpui::Pixels = px(22.);

fn caption(text: &str, theme: Theme) -> impl IntoElement {
    div()
        .px(metrics::SPACE_4)
        .pt(metrics::SPACE_4)
        .pb(metrics::SPACE_2)
        .text_size(metrics::TEXT_XS)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.ink_soft)
        .child(text.to_uppercase())
}

/// Une correction : case à cocher, règle, emplacement, « Ouvrir », points.
fn plan_row(
    gi: usize,
    report: &AnalysisReport,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> AnyElement {
    let group = &report.groups()[gi];
    let resolved = report.is_group_resolved(gi);
    let count = group.occurrences.len();
    let first = &report.issues[group.occurrences[0]];
    let files: std::collections::HashSet<&str> = group
        .occurrences
        .iter()
        .map(|&ix| report.issues[ix].file.as_ref())
        .collect();
    let location = match files.len() {
        0 | 1 => first.location().to_string(),
        n => format!("{} et {} autres fichiers", first.location(), n - 1),
    };
    let severity = format!(
        "{}{}",
        if group.review {
            "À revoir"
        } else {
            group.severity.label()
        },
        if count > 1 {
            format!(" ×{count}")
        } else {
            String::new()
        }
    );
    let severity_tone = if group.review {
        theme.ink_muted
    } else {
        severity_color(group.severity, &theme)
    };

    div()
        .id(("plan-row", gi))
        .flex()
        .items_center()
        .gap(metrics::SPACE_3)
        .px(metrics::SPACE_4)
        .py(metrics::SPACE_3)
        .border_t_1()
        .border_color(theme.border_soft)
        .when(resolved, |this| this.bg(theme.success_bg))
        .child(
            div()
                .id(("plan-check", gi))
                .flex_none()
                .size(CHECK_SIZE)
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .border_1()
                .cursor_pointer()
                .map(|this| {
                    if resolved {
                        this.bg(theme.success)
                            .border_color(theme.success)
                            .child(Icon::new(IconName::Check, theme.bg_surface).size(px(12.)))
                    } else {
                        this.bg(theme.bg_surface)
                            .border_color(theme.ink_soft)
                            .hover(|style| style.border_color(theme.ink))
                    }
                })
                .on_click(cx.listener(move |app, _, _, cx| app.toggle_group_resolved(gi, cx))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(metrics::SPACE_2)
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .font_weight(FontWeight::MEDIUM)
                                .when(resolved, |this| {
                                    this.text_color(theme.ink_soft).line_through()
                                })
                                .child(group.title.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(metrics::TEXT_XS)
                                .text_color(severity_tone)
                                .child(severity),
                        ),
                )
                .child(
                    div()
                        .truncate()
                        .font_family(fonts::MONO)
                        .text_size(metrics::TEXT_SM)
                        .text_color(theme.ink_soft)
                        .child(location),
                ),
        )
        .map(|this| {
            if resolved {
                this.child(
                    div()
                        .flex_none()
                        .text_size(metrics::TEXT_SM)
                        .text_color(theme.success)
                        .child("Corrigé, à confirmer"),
                )
            } else {
                this.child(
                    Button::new(("plan-open", gi), "Ouvrir", theme)
                        .on_click(cx.listener(move |app, _, _, cx| app.open_review(gi, cx))),
                )
            }
        })
        .child(
            div()
                .w(GAIN_WIDTH)
                .flex_none()
                .flex()
                .justify_end()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if resolved {
                    theme.success
                } else {
                    theme.accent_text
                })
                .child(short_points(report.base_gain(gi))),
        )
        .into_any_element()
}

/// Bouton icône qui réduit ou rouvre le panneau de la note.
fn panel_toggle(
    id: &'static str,
    icon: IconName,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(metrics::CONTROL_H)
        .rounded(metrics::RADIUS)
        .cursor_pointer()
        .hover(|style| style.bg(theme.bg_hover))
        .on_click(cx.listener(|app, _, _, cx| app.toggle_score_panel(cx)))
        .child(Icon::new(icon, theme.ink_muted))
}

/// Panneau réduit : la note et le score dans une bande, un clic le rouvre.
fn collapsed_aside(
    report: &AnalysisReport,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> AnyElement {
    let grade = Grade::from_score(report.projected_score());
    div()
        .id("score-panel-collapsed")
        .w(COLLAPSED_WIDTH)
        .flex_none()
        .h_full()
        .flex()
        .flex_col()
        .items_center()
        .gap(metrics::SPACE_3)
        .py(metrics::SPACE_5)
        .bg(theme.bg_surface)
        .border_l_1()
        .border_color(theme.border)
        .cursor_pointer()
        .hover(|style| style.bg(theme.bg_hover))
        .on_click(cx.listener(|app, _, _, cx| app.toggle_score_panel(cx)))
        .child(Icon::new(IconName::ChevronLeft, theme.ink_muted))
        .child(
            div()
                .font_family(fonts::DISPLAY)
                .font_weight(FontWeight::BOLD)
                .text_size(COLLAPSED_GRADE)
                .text_color(grade_color(grade, &theme))
                .child(grade.label()),
        )
        .child(
            div()
                .text_size(metrics::TEXT_XS)
                .text_color(theme.ink_soft)
                .child(report.projected_score().to_string()),
        )
        .into_any_element()
}

/// Colonne de droite : note actuelle → note estimée, barre, palier suivant.
fn score_aside(
    report: &AnalysisReport,
    order: &[usize],
    rerun: AnyElement,
    theme: Theme,
) -> impl IntoElement {
    let score = report.summary.score;
    let projected = report.projected_score();
    let current = Grade::from_score(score);
    let estimated = Grade::from_score(projected);
    let checked = order
        .iter()
        .filter(|&&gi| report.is_group_resolved(gi))
        .count();
    let next = report.next_grade();

    let big = |grade: Grade| {
        div()
            .font_family(fonts::DISPLAY)
            .font_weight(FontWeight::BOLD)
            .text_size(BIG_GRADE)
            .line_height(BIG_GRADE)
            .text_color(grade_color(grade, &theme))
            .child(grade.label())
    };
    let stat = |label: &'static str, value: String, color: gpui::Hsla| {
        div()
            .flex()
            .justify_between()
            .child(div().text_color(theme.ink_muted).child(label))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color)
                    .child(value),
            )
    };
    let summary: SharedString = if projected > score {
        format!("{score} aujourd'hui, {projected} estimés avec les corrections cochées.").into()
    } else {
        format!("{score} aujourd'hui. Cochez une correction pour voir la note estimée.").into()
    };

    div()
        .w(ASIDE_WIDTH)
        .flex_none()
        .h_full()
        .flex()
        .flex_col()
        .gap(metrics::SPACE_5)
        .px(metrics::SPACE_6)
        .py(metrics::SPACE_6)
        .bg(theme.bg_surface)
        .border_l_1()
        .border_color(theme.border)
        .child(
            div()
                .text_size(metrics::TEXT_XS)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.ink_soft)
                .child("VOTRE NOTE"),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(metrics::SPACE_3)
                .child(big(current))
                .when(estimated != current, |this| {
                    this.child(
                        div()
                            .text_size(metrics::TEXT_XL)
                            .text_color(theme.ink_soft)
                            .child("→"),
                    )
                    .child(big(estimated))
                }),
        )
        .child(div().text_color(theme.ink_muted).child(summary))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(metrics::SPACE_1)
                .child(
                    div()
                        .relative()
                        .h(BAR_H)
                        .rounded_full()
                        .overflow_hidden()
                        .bg(theme.track)
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .left_0()
                                .w(relative(f32::from(score) / 100.))
                                .bg(theme.accent),
                        )
                        .when(projected > score, |this| {
                            this.child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .left(relative(f32::from(score) / 100.))
                                    .w(relative(f32::from(projected - score) / 100.))
                                    .bg(theme.success),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .text_size(metrics::TEXT_XS)
                        .text_color(theme.ink_soft)
                        .child("0")
                        .child(format!("{score} aujourd'hui"))
                        .child("100"),
                ),
        )
        // Relancer l'analyse, juste sous la note.
        .child(div().flex().child(rerun))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(metrics::SPACE_2)
                .pt(metrics::SPACE_4)
                .border_t_1()
                .border_color(theme.border_soft)
                .child(stat(
                    "Corrections cochées",
                    format!("{checked} sur {}", order.len()),
                    theme.ink,
                ))
                .child(stat(
                    "Points en attente",
                    format!("+{}", projected.saturating_sub(score)),
                    theme.success,
                ))
                .child(stat(
                    "Prochain palier",
                    match next {
                        Some(next) => format!("{} dans {} pts", next.grade, next.points_needed),
                        None => "Note maximale".to_owned(),
                    },
                    theme.ink,
                )),
        )
        .child(div().flex_1())
        .child(
            div()
                .text_size(metrics::TEXT_SM)
                .text_color(theme.ink_soft)
                .child(
                    "Les corrections cochées sont confirmées à la prochaine analyse. \
                     Une correction non faite repasse dans la liste.",
                ),
        )
}

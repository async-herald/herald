//! Écran « Problèmes », vue Plan : les corrections triées par points
//! rapportés, à cocher, avec la note estimée qui monte dans la colonne de droite.

use gpui::{AnyElement, FontWeight, Window, div, prelude::*, px};

use super::analysis::{rerun_button, severity_color, short_points};
use super::app::{HeraldApp, WorkspaceView};
use super::components::{Button, ButtonVariant, Icon, IconName, grade_color};
use super::home::count_label;
use crate::export::Format;
use crate::model::{AnalysisReport, Category, Grade};
use crate::theme::{Theme, fonts, metrics};

/// Corrections proposées au plus dans « Gains rapides ».
const MAX_QUICK: usize = 5;
/// Largeur de fenêtre sous laquelle le panneau de la note se replie.
pub(super) const NARROW_WINDOW: gpui::Pixels = px(1200.);
const ASIDE_WIDTH: gpui::Pixels = px(320.);
/// Panneau « Votre note » réduit : une bande avec la note.
const COLLAPSED_WIDTH: gpui::Pixels = px(56.);
const COLLAPSED_GRADE: gpui::Pixels = px(26.);
/// Réglage mémorisé : panneau « Votre note » réduit.
pub(super) const SCORE_PANEL_COLLAPSED: &str = "score_panel_collapsed";
const CHECK_SIZE: gpui::Pixels = px(16.);
const GAIN_WIDTH: gpui::Pixels = px(56.);
const BIG_GRADE: gpui::Pixels = px(120.);

impl HeraldApp {
    /// Ouvre le groupe `gi` dans l'onglet Code, sur sa première occurrence.
    pub(super) fn open_group_in_code(
        &mut self,
        gi: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_group = Some(gi);
        self.selected_occurrence = 0;
        let target = self
            .selected_issue_index()
            .and_then(|ix| self.report()?.issues.get(ix))
            .map(|issue| (issue.file.to_string(), issue.line));
        match target {
            Some((file, line)) => self.open_in_ide(&file, line, window, cx),
            None => self.set_workspace_view(WorkspaceView::Code, window, cx),
        }
    }

    pub(super) fn render_correction_plan(
        &self,
        report: &AnalysisReport,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.project().is_none() {
            return div().into_any_element();
        }
        let filter = self.category_filter;
        // Filtré sur une catégorie (depuis l'onglet Général) : ses corrections
        // seulement, toujours triées par points, sans « Gains rapides ».
        let (order, quick) = match filter {
            None => report.correction_plan(MAX_QUICK),
            Some(category) => {
                let (order, _) = report.correction_plan(MAX_QUICK);
                let order: Vec<usize> = order
                    .into_iter()
                    .filter(|&gi| report.groups()[gi].category == category)
                    .collect();
                (order, 0)
            }
        };
        let current = report.summary.grade();
        let target_score = report.score_with(&order[..quick]);
        let target = Grade::from_score(target_score);

        let title = if let Some(category) = filter {
            if order.is_empty() {
                format!("{} · rien à corriger", category.label())
            } else {
                format!(
                    "{} · {}",
                    category.label(),
                    count_label(order.len() as u32, "correction", "corrections")
                )
            }
        } else if order.is_empty() {
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
            .when(quick > 0, |this| {
                this.child(caption("Gains rapides · triés par points rapportés", theme))
            })
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
                this.child(caption(
                    if quick > 0 {
                        "Ensuite"
                    } else {
                        "Corrections · triées par points rapportés"
                    },
                    theme,
                ))
                .children(rest_rows)
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
                            .gap(metrics::SPACE_5)
                            .px(metrics::SPACE_8)
                            .py(metrics::SPACE_6)
                            // Même en-tête que les autres onglets (nom, onglets).
                            .child(self.workspace_header(theme, cx))
                            .child(
                                div()
                                    .text_size(TITLE_SIZE)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .children(self.render_analysis_card(theme, cx))
                            .children(filter.map(|category| {
                                filter_notice(category, order.is_empty(), theme, cx)
                            }))
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
        // Fenêtre étroite : le panneau se replie de lui-même.
        if self.score_panel_collapsed || self.viewport_width < NARROW_WINDOW {
            return collapsed_aside(report, theme, cx);
        }
        let (order, _) = report.correction_plan(MAX_QUICK);
        // Relancer (action principale, toute la largeur), puis télécharger le
        // rapport (PDF ou Markdown).
        let actions = div()
            .flex()
            .flex_col()
            .gap(metrics::SPACE_2)
            .child(rerun_button(
                self.usage(),
                self.is_analyzing(&project.path),
                true,
                theme,
                cx,
            ))
            .child(
                div()
                    .flex()
                    .gap(metrics::SPACE_2)
                    .child(
                        div().flex_1().child(
                            Button::new("export-pdf", "Rapport PDF", theme)
                                .icon(IconName::Export)
                                .full_width()
                                .on_click(
                                    cx.listener(|app, _, _, cx| app.export_report(Format::Pdf, cx)),
                                ),
                        ),
                    )
                    .child(
                        div().flex_1().child(
                            Button::new("export-markdown", "Markdown", theme)
                                .full_width()
                                .on_click(cx.listener(|app, _, _, cx| {
                                    app.export_report(Format::Markdown, cx)
                                })),
                        ),
                    ),
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

/// Bandeau du Plan filtré : la catégorie, et « Tout afficher » pour retirer le filtre.
fn filter_notice(
    category: Category,
    empty: bool,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(metrics::SPACE_3)
        .px(metrics::SPACE_4)
        .py(metrics::SPACE_2)
        .rounded(metrics::RADIUS_LG)
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg_surface)
        .text_size(metrics::TEXT_SM)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_color(if empty {
                    theme.success
                } else {
                    theme.ink_muted
                })
                .child(if empty {
                    format!("Aucun problème dans la catégorie {}.", category.label())
                } else {
                    format!("Seulement la catégorie {}.", category.label())
                }),
        )
        .child(
            Button::new("plan-filter-clear", "Tout afficher", theme)
                .variant(ButtonVariant::Ghost)
                .on_click(cx.listener(|app, _, _, cx| {
                    app.category_filter = None;
                    cx.notify();
                })),
        )
}

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
                this.child(row_actions(gi, theme, cx))
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

/// Phrase du panneau de la note : l'évolution, puis le palier suivant et la
/// correction qui suffit à l'atteindre, ou l'effet des corrections cochées.
fn score_sentence(
    delta: Option<i16>,
    next: Option<(&str, u8)>,
    enough: Option<(&str, usize)>,
    estimated: Option<(&str, u8)>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    match delta {
        Some(0) => parts.push("Stable.".to_owned()),
        Some(d) if d > 0 => parts.push(format!(
            "+{d} point{} depuis l'analyse précédente.",
            if d > 1 { "s" } else { "" }
        )),
        Some(d) => parts.push(format!(
            "−{} point{} depuis l'analyse précédente.",
            d.unsigned_abs(),
            if d.unsigned_abs() > 1 { "s" } else { "" }
        )),
        None => {}
    }
    if let Some((grade, score)) = estimated {
        parts.push(format!("Avec les corrections cochées : {grade}, {score}."));
    } else {
        match (next, enough) {
            (Some((grade, n)), Some((rule, count))) => parts.push(format!(
                "Encore {n} point{} pour {grade} : corriger « {rule} » ({count}) suffit.",
                if n > 1 { "s" } else { "" }
            )),
            (Some((grade, n)), None) => parts.push(format!(
                "Encore {n} point{} pour {grade}.",
                if n > 1 { "s" } else { "" }
            )),
            (None, _) => parts.push("La meilleure note est atteinte.".to_owned()),
        }
    }
    parts.join(" ")
}

/// Panneau de la note, minimal : la lettre en très grand, la note sur 100,
/// une phrase, puis la relance et les exports en bas.
fn score_aside(
    report: &AnalysisReport,
    order: &[usize],
    rerun: AnyElement,
    theme: Theme,
) -> impl IntoElement {
    let score = report.summary.score;
    let projected = report.projected_score();
    let current = Grade::from_score(score);
    let next = report.next_grade();
    // La première correction du plan qui, seule, atteint le palier suivant.
    let enough = next.and_then(|next| {
        order
            .iter()
            .copied()
            .filter(|&gi| !report.is_group_resolved(gi))
            .find(|&gi| report.base_gain(gi) >= f32::from(next.points_needed))
            .map(|gi| {
                let group = &report.groups()[gi];
                (group.title.to_string(), group.occurrences.len())
            })
    });
    let estimated_label = Grade::from_score(projected).label();
    let sentence = score_sentence(
        report.summary.delta(),
        next.map(|n| (n.grade.label(), n.points_needed)),
        enough.as_ref().map(|(t, n)| (t.as_str(), *n)),
        (projected > score).then_some((estimated_label, projected)),
    );

    div()
        .w(ASIDE_WIDTH)
        .flex_none()
        .h_full()
        .flex()
        .flex_col()
        .px(metrics::SPACE_6)
        .py(metrics::SPACE_6)
        .bg(theme.bg_surface)
        .border_l_1()
        .border_color(theme.border)
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(metrics::SPACE_3)
                .child(
                    div()
                        .font_family(fonts::DISPLAY)
                        .font_weight(FontWeight::BOLD)
                        .text_size(BIG_GRADE)
                        .line_height(BIG_GRADE)
                        .text_color(grade_color(current, &theme))
                        .child(current.label()),
                )
                .child(
                    div()
                        .text_size(metrics::TEXT_LG)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("{score} sur 100")),
                )
                .child(
                    div()
                        .max_w(px(240.))
                        .text_center()
                        .text_size(metrics::TEXT_SM)
                        .line_height(px(21.))
                        .text_color(theme.ink_muted)
                        .child(sentence),
                ),
        )
        .child(rerun)
}

/// « Faux positif » (ouvre la modale de signalement) et « Ouvrir ».
fn row_actions(gi: usize, theme: Theme, cx: &mut Context<HeraldApp>) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(metrics::SPACE_2)
        .child(
            div()
                .id(("plan-false-positive", gi))
                .flex()
                .items_center()
                .gap(metrics::SPACE_2)
                .h(metrics::CONTROL_H)
                .px(metrics::SPACE_3)
                .rounded(metrics::RADIUS)
                .border_1()
                .border_color(theme.border)
                .bg(theme.bg_surface)
                .text_size(metrics::TEXT_SM)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.ink_muted)
                .cursor_pointer()
                .hover(|style| style.bg(theme.bg_hover))
                .on_click(
                    cx.listener(move |app, _, window, cx| app.open_false_positive(gi, window, cx)),
                )
                .child(Icon::new(IconName::Flag, theme.ink_muted).size(metrics::ICON_SM))
                .child("Faux positif"),
        )
        .child(
            Button::new(("plan-open", gi), "Ouvrir", theme)
                .icon(IconName::ArrowRight)
                .on_click(
                    cx.listener(move |app, _, window, cx| app.open_group_in_code(gi, window, cx)),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::score_sentence;

    #[test]
    fn the_score_panel_says_it_in_one_sentence() {
        assert_eq!(
            score_sentence(
                Some(0),
                Some(("A+", 2)),
                Some(("Ligne trop longue", 13)),
                None
            ),
            "Stable. Encore 2 points pour A+ : corriger « Ligne trop longue » (13) suffit."
        );
        assert_eq!(
            score_sentence(Some(6), Some(("A+", 1)), None, None),
            "+6 points depuis l'analyse précédente. Encore 1 point pour A+."
        );
        assert_eq!(
            score_sentence(None, Some(("A+", 2)), None, Some(("A+", 98))),
            "Avec les corrections cochées : A+, 98."
        );
        assert_eq!(
            score_sentence(Some(-1), None, None, None),
            "−1 point depuis l'analyse précédente. La meilleure note est atteinte."
        );
    }
}

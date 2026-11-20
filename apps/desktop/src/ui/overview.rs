//! « Vue d'ensemble » : une carte par projet (note, problèmes, alertes CVE,
//! dernière analyse) et une carte pour en ajouter un. Un clic ouvre le projet.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, FontWeight, SharedString, div, prelude::*,
    pulsating_between, px,
};

use super::app::HeraldApp;
use super::components::{Button, ButtonVariant, Icon, IconName, ProgressBar, grade_color};
use super::home::count_label;
use crate::model::{Project, Severity};
use crate::theme::{Theme, fonts, metrics};

/// Côté d'une carte : les cartes sont carrées.
const CARD_SIZE: gpui::Pixels = px(232.);
const GRADE_SIZE: gpui::Pixels = px(44.);
const STAT_SIZE: gpui::Pixels = px(30.);
const STAT_MIN_WIDTH: gpui::Pixels = px(96.);

impl HeraldApp {
    /// Ouvre la page d'un projet.
    pub(super) fn open_project(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.projects.len() {
            return;
        }
        if ix != self.selected_project {
            self.selected_project = ix;
            self.reset_selection();
        }
        self.screen = self.project_screen();
        cx.notify();
    }

    pub(super) fn render_overview(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        if self.projects.is_empty() {
            return self.render_project_page(theme, cx);
        }

        let analyzed: Vec<u8> = self
            .projects
            .iter()
            .filter_map(|project| project.report.as_ref().map(|r| r.summary.score))
            .collect();
        let average = (!analyzed.is_empty())
            .then(|| analyzed.iter().map(|&s| u32::from(s)).sum::<u32>() / analyzed.len() as u32);
        let unread = self.unread_alert_count();
        let stat = |value: String, label: String, color: gpui::Hsla| {
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .min_w(STAT_MIN_WIDTH)
                .child(
                    div()
                        .font_family(fonts::DISPLAY)
                        .font_weight(FontWeight::BOLD)
                        .text_size(STAT_SIZE)
                        .line_height(STAT_SIZE)
                        .text_color(color)
                        .child(value),
                )
                .child(
                    div()
                        .text_size(metrics::TEXT_SM)
                        .text_color(theme.ink_muted)
                        .child(label),
                )
        };
        let stats = div()
            .flex()
            .flex_wrap()
            .gap(metrics::SPACE_8)
            .child(stat(
                self.projects.len().to_string(),
                if self.projects.len() > 1 {
                    "projets"
                } else {
                    "projet"
                }
                .to_owned(),
                theme.ink,
            ))
            .child(stat(
                analyzed.len().to_string(),
                if analyzed.len() > 1 {
                    "analysés"
                } else {
                    "analysé"
                }
                .to_owned(),
                theme.ink,
            ))
            .when_some(average, |this, average| {
                this.child(stat(
                    format!("{average}"),
                    "note moyenne sur 100".to_owned(),
                    grade_color(crate::model::Grade::from_score(average as u8), &theme),
                ))
            })
            .child(stat(
                unread.to_string(),
                if unread > 1 {
                    "alertes CVE non lues"
                } else {
                    "alerte CVE non lue"
                }
                .to_owned(),
                if unread > 0 { theme.danger } else { theme.ink },
            ));

        let cards = self
            .projects
            .iter()
            .enumerate()
            .map(|(ix, project)| self.project_card(ix, project, theme, cx))
            .collect::<Vec<_>>();

        div()
            .flex()
            .flex_col()
            .gap(metrics::SPACE_6)
            .child(
                div()
                    .text_size(metrics::TEXT_XL)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Vue d'ensemble"),
            )
            .child(stats)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(metrics::SPACE_4)
                    .children(cards)
                    .child(add_card(theme, cx)),
            )
            .into_any_element()
    }

    fn project_card(
        &self,
        ix: usize,
        project: &Project,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let analyzing = self.is_analyzing(&project.path);
        let alerts = self
            .alerts
            .iter()
            .filter(|alert| alert.project_path == project.path)
            .count();
        let group = SharedString::from(format!("project-card-{ix}"));

        let snapshot = self.run_snapshot(&project.path);
        let body: AnyElement = if let Some(run) = &snapshot {
            running_body(run, theme)
        } else {
            match &project.report {
                Some(report) => {
                    let grade = report.summary.grade();
                    let color = grade_color(grade, &theme);
                    let critical = report
                        .issues
                        .iter()
                        .filter(|issue| issue.severity == Severity::Critical)
                        .count();
                    let delta = report.summary.delta().filter(|d| *d != 0).map(|d| {
                        div()
                            .text_size(metrics::TEXT_SM)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if d > 0 { theme.success } else { theme.danger })
                            .child(format!("{d:+}"))
                    });
                    let mut issues =
                        count_label(report.issues.len() as u32, "problème", "problèmes");
                    if critical > 0 {
                        issues.push_str(&format!(
                            " · {}",
                            count_label(critical as u32, "critique", "critiques")
                        ));
                    }

                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .justify_end()
                        .gap(metrics::SPACE_2)
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap(metrics::SPACE_2)
                                .child(
                                    div()
                                        .font_family(fonts::DISPLAY)
                                        .font_weight(FontWeight::BOLD)
                                        .text_size(GRADE_SIZE)
                                        .line_height(GRADE_SIZE)
                                        .text_color(color)
                                        .child(grade.label()),
                                )
                                .child(
                                    div()
                                        .text_color(theme.ink_muted)
                                        .child(format!("{} / 100", report.summary.score)),
                                )
                                .children(delta),
                        )
                        .child(ProgressBar::new(
                            f32::from(report.summary.score) / 100.,
                            theme,
                        ))
                        .child(div().text_size(metrics::TEXT_SM).child(issues))
                        .when(alerts > 0, |this| {
                            this.child(
                                div()
                                    .text_size(metrics::TEXT_SM)
                                    .text_color(theme.danger)
                                    .child(count_label(alerts as u32, "alerte CVE", "alertes CVE")),
                            )
                        })
                        .child(
                            div()
                                .text_size(metrics::TEXT_SM)
                                .text_color(theme.ink_soft)
                                .child(if analyzing {
                                    "Analyse en cours…".to_owned()
                                } else {
                                    format!("analysé {}", report.summary.analyzed_at)
                                }),
                        )
                        .into_any_element()
                }
                None => div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .justify_end()
                    .gap(metrics::SPACE_3)
                    .child(div().text_color(theme.ink_muted).child(if analyzing {
                        "Analyse en cours…"
                    } else {
                        "Pas encore analysé"
                    }))
                    .when(!analyzing, |this| {
                        this.child(
                            Button::new(("card-analyze", ix), "Lancer l'analyse", theme)
                                .variant(ButtonVariant::Primary)
                                .icon(IconName::Play)
                                .on_click(cx.listener(move |app, _, _, cx| {
                                    cx.stop_propagation();
                                    app.analyze_project_at(ix, false, cx);
                                })),
                        )
                    })
                    .into_any_element(),
            }
        };

        let corner: AnyElement = if analyzing {
            super::analysis_run::spinner(theme).into_any_element()
        } else if project.report.is_some() {
            div()
                .id(("card-rerun", ix))
                .flex_none()
                .p(metrics::SPACE_1)
                .rounded(metrics::RADIUS)
                .opacity(0.5)
                .group_hover(group.clone(), |style| style.opacity(1.))
                .hover(|style| style.bg(theme.bg_hover))
                .on_click(cx.listener(move |app, _, _, cx| {
                    cx.stop_propagation();
                    app.analyze_project_at(ix, false, cx);
                }))
                .child(Icon::new(IconName::Refresh, theme.ink_muted).size(metrics::ICON_SM))
                .into_any_element()
        } else {
            div().into_any_element()
        };

        let card = div()
            .id(("project-card", ix))
            .group(group)
            .size(CARD_SIZE)
            .flex()
            .flex_col()
            .gap(metrics::SPACE_1)
            .p(metrics::SPACE_4)
            .rounded(metrics::RADIUS_LG)
            .bg(theme.bg_surface)
            .border_1()
            .border_color(if ix == self.selected_project {
                theme.accent
            } else {
                theme.border
            })
            .cursor_pointer()
            .hover(|style| style.border_color(theme.ink_soft))
            .on_click(cx.listener(move |app, _, _, cx| app.open_project(ix, cx)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(metrics::SPACE_2)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(metrics::TEXT_MD)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(project.name.clone()),
                    )
                    .child(corner),
            )
            .child(
                div()
                    .truncate()
                    .font_family(fonts::MONO)
                    .text_size(metrics::TEXT_XS)
                    .text_color(theme.ink_soft)
                    .child(project.display_path()),
            )
            .child(body);
        if analyzing {
            // Bordure dorée qui « respire » pendant l'analyse.
            let accent = theme.accent;
            card.with_animation(
                ("project-card-pulse", ix),
                Animation::new(Duration::from_millis(1_600))
                    .repeat()
                    .with_easing(pulsating_between(0.25, 1.)),
                move |card, delta| card.border_color(accent.opacity(delta)),
            )
            .into_any_element()
        } else {
            card.into_any_element()
        }
    }
}

/// Carte pendant l'analyse : pourcentage, étape, barre et fichier lu.
fn running_body(run: &super::analysis_run::RunSnapshot, theme: Theme) -> AnyElement {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .justify_end()
        .gap(metrics::SPACE_2)
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(px(2.))
                .child(
                    div()
                        .font_family(fonts::DISPLAY)
                        .font_weight(FontWeight::BOLD)
                        .text_size(GRADE_SIZE)
                        .line_height(GRADE_SIZE)
                        .text_color(theme.accent_text)
                        .child(run.percent.to_string()),
                )
                .child(
                    div()
                        .text_size(metrics::TEXT_LG)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.accent_text)
                        .child("%"),
                ),
        )
        .child(ProgressBar::new(run.percent as f32 / 100., theme))
        .child(
            div()
                .truncate()
                .text_size(metrics::TEXT_SM)
                .child(run.step.clone()),
        )
        .child(
            div()
                .truncate()
                .font_family(fonts::MONO)
                .text_size(metrics::TEXT_XS)
                .text_color(theme.ink_soft)
                .child(
                    run.file
                        .clone()
                        .unwrap_or_else(|| "Analyse locale · aucun fichier envoyé".into()),
                ),
        )
        .into_any_element()
}

/// Carte « Ajouter un projet ».
fn add_card(theme: Theme, cx: &mut Context<HeraldApp>) -> impl IntoElement {
    div()
        .id("project-card-add")
        .size(CARD_SIZE)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(metrics::SPACE_2)
        .rounded(metrics::RADIUS_LG)
        .border_1()
        .border_dashed()
        .border_color(theme.border)
        .text_color(theme.ink_muted)
        .cursor_pointer()
        .hover(|style| style.bg(theme.bg_hover).text_color(theme.ink))
        .on_click(cx.listener(|app, _, window, cx| app.open_new_project(window, cx)))
        .child(Icon::new(IconName::Plus, theme.ink_muted))
        .child("Ajouter un projet")
}

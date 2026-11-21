//! Vue d'ensemble : prise en main, note du projet, gains rapides.

use gpui::{AnyElement, Div, ExternalPaths, FontWeight, SharedString, div, prelude::*, px};

use super::app::HeraldApp;
use super::components::{
    Button, ButtonVariant, GradeText, HeraldEmblem, Icon, IconName, ProgressBar, SeverityLabel,
    grade_color,
};
use super::new_project::Kind;
use crate::model::{AnalysisReport, Project, format_points};
use crate::theme::{Theme, fonts, metrics};

const CONTENT_MAX_WIDTH: gpui::Pixels = px(960.);
const SCORE_COLUMN_WIDTH: gpui::Pixels = px(240.);
const LOCATION_WIDTH: gpui::Pixels = px(220.);
const LABEL_WIDTH: gpui::Pixels = px(96.);
const POINTS_WIDTH: gpui::Pixels = px(56.);
const WELCOME_EMBLEM_SIZE: gpui::Pixels = px(170.);
const WELCOME_TITLE_SIZE: gpui::Pixels = px(26.);
const DROP_ZONE_HEIGHT: gpui::Pixels = px(260.);
const QUICK_WIN_LIMIT: usize = 5;

impl HeraldApp {
    pub(super) fn render_project_page(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let content = div()
            .max_w(CONTENT_MAX_WIDTH)
            .flex()
            .flex_col()
            .gap(metrics::SPACE_6);

        let Some(project) = self.project() else {
            return content
                .child(self.render_welcome(theme, cx))
                .into_any_element();
        };

        let main_panel = match &project.report {
            Some(report) => score_panel(report, theme).into_any_element(),
            None => surface(theme)
                .p(metrics::SPACE_5)
                .flex()
                .items_center()
                .gap(metrics::SPACE_5)
                .child(HeraldEmblem::new(px(72.), theme))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(metrics::SPACE_1)
                        .child(
                            div()
                                .text_size(metrics::TEXT_LG)
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Prêt pour l'analyse"),
                        )
                        .child(div().text_color(theme.ink_muted).child(
                            "Herald relit le code sur cette machine et note le projet sur 100.",
                        ))
                        .child(
                            div()
                                .text_size(metrics::TEXT_SM)
                                .text_color(theme.ink_soft)
                                .child("100 % local · Sans IA · aucun fichier envoyé"),
                        ),
                )
                // Pendant l'analyse, la carte au-dessus suffit.
                .when(!self.is_analyzing(&project.path), |this| {
                    this.child(
                        Button::new("start-analysis", "Lancer l'analyse", theme)
                            .variant(ButtonVariant::Primary)
                            .icon(IconName::Play)
                            .large()
                            .on_click(cx.listener(|app, _, _, cx| app.start_analysis(cx))),
                    )
                })
                .into_any_element(),
        };
        let quick_wins = project
            .report
            .as_ref()
            .map(|report| self.render_quick_wins(report, theme, cx));

        content
            .child(self.render_overview_header(project, theme, cx))
            .children(self.render_analysis_card(theme, cx))
            .children(self.render_alert_notice(theme, cx))
            .child(main_panel)
            .children(quick_wins)
            .child(drop_hint(theme, cx))
            .into_any_element()
    }

    fn render_overview_header(
        &self,
        project: &Project,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let mut meta = vec![project.display_path().to_string()];
        if let Some(report) = &project.report {
            meta.push(format!("analysé {}", report.summary.analyzed_at));
        }

        div()
            .flex()
            .items_end()
            .justify_between()
            .gap(metrics::SPACE_4)
            .child(page_title(project.name.clone(), meta.join(" · "), theme))
            .when(project.report.is_some(), |this| {
                this.child(
                    div()
                        .flex()
                        .flex_none()
                        .gap(metrics::SPACE_2)
                        .child(super::analysis::rerun_button(
                            self.usage(),
                            self.is_analyzing(&project.path),
                            false,
                            theme,
                            cx,
                        ))
                        .child(
                            Button::new("open-analysis", "Voir les problèmes", theme)
                                .variant(ButtonVariant::Primary)
                                .on_click(cx.listener(|app, _, _, cx| app.open_analysis(cx))),
                        ),
                )
            })
    }

    /// Ligne discrète tant que des alertes CVE n'ont pas été lues.
    fn render_alert_notice(&self, theme: Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let unread = self.unread_alert_count();
        if !super::app::ALERTS_ENABLED || unread == 0 {
            return None;
        }
        let text = if unread == 1 {
            "1 nouvelle alerte CVE concerne une dépendance de vos projets.".to_owned()
        } else {
            format!("{unread} nouvelles alertes CVE concernent des dépendances de vos projets.")
        };

        Some(
            surface(theme)
                .flex()
                .items_center()
                .gap(metrics::SPACE_3)
                .px(metrics::SPACE_4)
                .py(metrics::SPACE_2)
                .child(Icon::new(IconName::Bell, theme.danger).size(metrics::ICON_SM))
                .child(div().flex_1().child(text))
                .child(
                    Button::new("see-alerts", "Voir les alertes", theme)
                        .variant(ButtonVariant::Ghost)
                        .on_click(cx.listener(|app, _, _, cx| app.open_alerts(cx))),
                )
                .into_any_element(),
        )
    }

    /// Accueil sans projet : l'ange au-dessus d'une grande zone où confier un dossier.
    fn render_welcome(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let link = |id: &'static str, label: &'static str, kind: Kind| {
            div()
                .id(id)
                .text_color(theme.accent_text)
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .hover(|style| style.underline())
                .on_click(cx.listener(move |app, _, window, cx| {
                    cx.stop_propagation();
                    app.start_project_kind(kind, window, cx)
                }))
                .child(label)
        };
        let dot = || div().text_color(theme.ink_soft).child("·");

        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(metrics::SPACE_5)
            .pt(metrics::SPACE_4)
            .child(HeraldEmblem::new(WELCOME_EMBLEM_SIZE, theme))
            .child(
                div()
                    .id("welcome-drop-zone")
                    .w_full()
                    .h(DROP_ZONE_HEIGHT)
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(metrics::SPACE_3)
                    .rounded(px(18.))
                    .border_2()
                    .border_dashed()
                    .border_color(theme.accent.opacity(0.6))
                    .bg(theme.bg_surface.opacity(0.6))
                    .cursor_pointer()
                    .hover(move |style| style.bg(theme.bg_surface))
                    // Un dossier glissé au-dessus : la zone s'illumine.
                    .drag_over::<ExternalPaths>(move |style, _, _, _| {
                        style.bg(theme.bg_active).border_color(theme.accent)
                    })
                    .on_click(cx.listener(|app, _, _, cx| app.pick_folder(cx)))
                    .child(Icon::new(IconName::FolderUp, theme.accent_text).size(px(28.)))
                    .child(
                        div()
                            .font_family(fonts::DISPLAY)
                            .font_weight(FontWeight::BOLD)
                            .text_size(WELCOME_TITLE_SIZE)
                            .child("Déposez un dossier ici"),
                    )
                    .child(div().text_color(theme.ink_muted).child(
                        "Herald le relit sur cette machine et vous dit quoi corriger en premier.",
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(metrics::SPACE_2)
                            .pt(metrics::SPACE_2)
                            .child(div().text_color(theme.ink_soft).child("ou"))
                            .child(link("welcome-local", "choisissez un dossier", Kind::Local))
                            .child(dot())
                            .child(link("welcome-git", "clonez un dépôt Git", Kind::Git))
                            .child(dot())
                            .child(link("welcome-demo", "essayez la démo", Kind::Demo)),
                    ),
            )
            .child(
                div()
                    .text_size(metrics::TEXT_SM)
                    .text_color(theme.ink_soft)
                    .child("100 % local · Sans IA · 30+ langages"),
            )
    }

    fn render_quick_wins(
        &self,
        report: &AnalysisReport,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if report.issues.is_empty() {
            return div()
                .text_size(metrics::TEXT_SM)
                .text_color(theme.ink_soft)
                .child(format!(
                    "{} à la dernière analyse. Détail non disponible pour ce projet de démo.",
                    count_label(report.summary.issue_count, "problème", "problèmes")
                ))
                .into_any_element();
        }

        let rows =
            report.quick_wins(QUICK_WIN_LIMIT).into_iter().map(|gi| {
                let group = &report.groups()[gi];
                let issue = &report.issues[group.occurrences[0]];
                list_row(theme)
                    .id(("quick-win", gi))
                    .items_center()
                    .py(metrics::SPACE_2)
                    .cursor_pointer()
                    .hover(|style| style.bg(theme.bg_hover))
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.select_group(gi, cx);
                        app.open_analysis(cx);
                    }))
                    .child(
                        div()
                            .w(LABEL_WIDTH)
                            .flex_none()
                            .child(SeverityLabel::new(group.severity, theme)),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(
                        if group.occurrences.len() > 1 {
                            format!("{} (× {})", group.title, group.occurrences.len())
                        } else {
                            group.title.to_string()
                        },
                    ))
                    .child(
                        div()
                            .w(LOCATION_WIDTH)
                            .flex_none()
                            .truncate()
                            .font_family(fonts::MONO)
                            .text_size(metrics::TEXT_SM)
                            .text_color(theme.ink_soft)
                            .child(issue.location()),
                    )
                    .child(points(report.group_gain(gi), theme))
            });

        surface(theme)
            .flex()
            .flex_col()
            .child(
                section_header(
                    "Gains rapides",
                    "corrections qui font le plus monter la note",
                    theme,
                )
                .child(
                    Button::new("all-issues", "Tout voir", theme)
                        .variant(ButtonVariant::Ghost)
                        .on_click(cx.listener(|app, _, _, cx| app.open_analysis(cx))),
                ),
            )
            .children(rows)
            .into_any_element()
    }
}

/// Note du projet à gauche, détail par catégorie à droite, dans une seule surface.
fn score_panel(report: &AnalysisReport, theme: Theme) -> impl IntoElement {
    let summary = &report.summary;
    let grade = summary.grade();

    let score = div()
        .w(SCORE_COLUMN_WIDTH)
        .flex_none()
        .flex()
        .flex_col()
        .gap(metrics::SPACE_2)
        .p(metrics::SPACE_5)
        .border_r_1()
        .border_color(theme.border_soft)
        .child(
            div()
                .flex()
                .items_end()
                .gap(metrics::SPACE_3)
                .child(
                    div()
                        .font_family(fonts::DISPLAY)
                        .font_weight(FontWeight::BOLD)
                        .text_size(metrics::TEXT_SCORE)
                        .line_height(metrics::TEXT_SCORE)
                        .text_color(grade_color(grade, &theme))
                        .child(grade.label()),
                )
                .child(
                    div()
                        .pb(metrics::SPACE_1)
                        .flex()
                        .gap(metrics::SPACE_2)
                        .child(format!("{} / 100", summary.score))
                        .children(summary.delta().filter(|delta| *delta != 0).map(|delta| {
                            div()
                                .text_color(if delta >= 0 {
                                    theme.success
                                } else {
                                    theme.danger
                                })
                                .child(format!("{delta:+}"))
                        })),
                ),
        )
        .child(ProgressBar::new(
            f32::from(report.projected_score()) / 100.,
            theme,
        ))
        .child(
            div()
                .text_size(metrics::TEXT_SM)
                .text_color(theme.ink_muted)
                .child(match report.next_grade() {
                    Some(next) => format!("{} pts avant {}", next.points_needed, next.grade),
                    None => "Note maximale".to_owned(),
                }),
        );

    let categories = div()
        .flex_1()
        .flex()
        .flex_col()
        .justify_center()
        .gap(metrics::SPACE_3)
        .px(metrics::SPACE_6)
        .py(metrics::SPACE_4)
        .children(report.categories.iter().map(|category| {
            div()
                .flex()
                .items_center()
                .gap(metrics::SPACE_4)
                .child(
                    div()
                        .w(LABEL_WIDTH)
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(metrics::SPACE_1)
                        .child(category.category.label()),
                )
                .child(div().flex_1().child(ProgressBar::new(
                    if category.applicable {
                        f32::from(category.score) / 100.
                    } else {
                        0.
                    },
                    theme,
                )))
                .child(div().w(metrics::SPACE_8).flex().justify_end().child(
                    if category.applicable {
                        GradeText::new(Some(category.grade()), theme).into_any_element()
                    } else {
                        // Rien à examiner (ni Dockerfile/CI, ni tests) : hors de la note.
                        div()
                            .text_size(metrics::TEXT_SM)
                            .text_color(theme.ink_soft)
                            .child("N/A")
                            .into_any_element()
                    },
                ))
        }));

    surface(theme).flex().child(score).child(categories)
}

/// Ligne discrète en bas de la vue d'ensemble : ajouter un autre projet.
fn drop_hint(theme: Theme, cx: &mut Context<HeraldApp>) -> impl IntoElement {
    div()
        .flex()
        .justify_center()
        .gap(metrics::SPACE_1)
        .pt(metrics::SPACE_2)
        .text_size(metrics::TEXT_SM)
        .text_color(theme.ink_soft)
        .child("Déposez un autre dossier dans la fenêtre pour l'analyser, ou")
        .child(
            div()
                .id("hint-new-project")
                .text_color(theme.accent_text)
                .cursor_pointer()
                .hover(|style| style.underline())
                .on_click(cx.listener(|app, _, window, cx| app.open_new_project(window, cx)))
                .child("ajoutez un projet"),
        )
}

/// Titre d'écran avec sa ligne de métadonnées.
pub(super) fn page_title(
    title: impl Into<SharedString>,
    meta: impl Into<SharedString>,
    theme: Theme,
) -> impl IntoElement {
    div()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(metrics::SPACE_1)
        .child(
            div()
                .text_size(metrics::TEXT_XL)
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.into()),
        )
        .child(
            div()
                .truncate()
                .text_size(metrics::TEXT_SM)
                .text_color(theme.ink_muted)
                .child(meta.into()),
        )
}

/// En-tête d'une surface : titre, précision discrète, puis actions ajoutées par l'appelant.
pub(super) fn section_header(
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    theme: Theme,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap(metrics::SPACE_3)
        .px(metrics::SPACE_4)
        .py(metrics::SPACE_3)
        .child(
            div()
                .text_size(metrics::TEXT_MD)
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.into()),
        )
        .child(
            div()
                .flex_1()
                .text_size(metrics::TEXT_SM)
                .text_color(theme.ink_soft)
                .child(detail.into()),
        )
}

/// Ligne de liste pleine largeur, séparée de la précédente par un filet.
pub(super) fn list_row(theme: Theme) -> Div {
    div()
        .flex()
        .gap(metrics::SPACE_3)
        .px(metrics::SPACE_4)
        .py(metrics::SPACE_3)
        .border_t_1()
        .border_color(theme.border_soft)
}

/// Points gagnés en corrigeant un problème, alignés à droite.
pub(super) fn points(points: f32, theme: Theme) -> impl IntoElement {
    div()
        .w(POINTS_WIDTH)
        .flex_none()
        .flex()
        .justify_end()
        .text_size(metrics::TEXT_SM)
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.accent_text)
        .child(format_points(points))
}

/// Surface de travail : fond blanc, bordure, rayon. Jamais imbriquée.
pub(super) fn surface(theme: Theme) -> Div {
    div()
        .rounded(metrics::RADIUS_LG)
        .overflow_hidden()
        .bg(theme.bg_surface)
        .border_1()
        .border_color(theme.border)
}

/// « 1 problème », « 3 problèmes » (singulier jusqu'à 1, comme en français).
pub(super) fn count_label(count: u32, singular: &str, plural: &str) -> String {
    format!(
        "{} {}",
        thousands(count),
        if count <= 1 { singular } else { plural }
    )
}

/// Nombre avec ses milliers séparés à la française : « 19 357 ».
pub(super) fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (ix, c) in digits.chars().enumerate() {
        if ix > 0 && (digits.len() - ix) % 3 == 0 {
            out.push('\u{202F}');
        }
        out.push(c);
    }
    out
}

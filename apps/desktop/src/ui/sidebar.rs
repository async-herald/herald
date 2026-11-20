//! Barre latérale commune : navigation, projets, offre et quota.

use gpui::{ClickEvent, FontWeight, SharedString, Window, div, prelude::*, px};

use super::app::{ALERTS_ENABLED, HeraldApp, Screen};
use super::components::{GradeText, Icon, IconName, Logo, ProgressBar};
use crate::theme::{Theme, fonts, metrics};

const SIDEBAR_WIDTH: gpui::Pixels = px(232.);
const LOGO_SIZE: gpui::Pixels = px(26.);

impl HeraldApp {
    pub(super) fn render_sidebar(
        &self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let has_report = self.report().is_some();
        let unread = self.unread_alert_count();
        let issue_count = self
            .report()
            .map(|report| report.issues.len())
            .filter(|&count| count > 0);

        let nav = div()
            .flex()
            .flex_col()
            .child(nav_item(
                "nav-home",
                "Vue d'ensemble",
                self.screen == Screen::Home,
                (!self.projects.is_empty()).then_some(self.projects.len()),
                theme,
                cx.listener(|app, _, _, cx| app.go_home(cx)),
            ))
            .when(has_report, |this| {
                this.child(nav_item(
                    "nav-issues",
                    "Problèmes",
                    self.screen == Screen::Analysis,
                    issue_count,
                    theme,
                    cx.listener(|app, _, _, cx| app.open_analysis(cx)),
                ))
            })
            .child(nav_item(
                "nav-rules",
                "Règles",
                self.screen == Screen::Rules,
                None,
                theme,
                cx.listener(|app, _, _, cx| app.open_rules(cx)),
            ))
            .when(ALERTS_ENABLED, |this| {
                this.child(nav_item(
                    "nav-alerts",
                    "Alertes CVE",
                    self.screen == Screen::Alerts,
                    (unread > 0).then_some(unread),
                    theme,
                    cx.listener(|app, _, _, cx| app.open_alerts(cx)),
                ))
            })
            .child(nav_item(
                "nav-settings",
                "Réglages",
                self.screen == Screen::Settings,
                None,
                theme,
                cx.listener(|app, _, _, cx| app.open_settings(cx)),
            ));

        div()
            .w(SIDEBAR_WIDTH)
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .gap(metrics::SPACE_6)
            .px(metrics::SPACE_3)
            .py(metrics::SPACE_4)
            .bg(theme.bg_page)
            .border_r_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(metrics::SPACE_2)
                    .px(metrics::SPACE_2)
                    .child(Logo::new(LOGO_SIZE))
                    .child(
                        div()
                            .font_family(fonts::DISPLAY)
                            .font_weight(FontWeight::BOLD)
                            .text_size(metrics::TEXT_MD)
                            .child("Async Herald"),
                    ),
            )
            .child(nav)
            .child(self.render_project_list(theme, cx))
            .child(div().flex_1())
            .child(self.render_plan(theme, cx))
    }

    fn render_project_list(
        &self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let rows = self.projects.iter().enumerate().map(|(ix, project)| {
            let selected = ix == self.selected_project;
            let group = SharedString::from(format!("project-{ix}"));
            div()
                .id(("project", ix))
                .group(group.clone())
                .flex()
                .items_center()
                .gap(metrics::SPACE_2)
                .h(metrics::CONTROL_H)
                .px(metrics::SPACE_2)
                .rounded(metrics::RADIUS)
                .cursor_pointer()
                .when(selected, |this| this.bg(theme.bg_active))
                .when(!selected, |this| {
                    this.hover(|style| style.bg(theme.bg_hover))
                })
                .on_click(cx.listener(move |app, _, _, cx| {
                    // Depuis l'écran Problèmes on reste sur les problèmes du projet choisi ;
                    // ailleurs, on ouvre sa page.
                    if app.screen == Screen::Analysis {
                        app.select_project(ix, cx);
                    } else {
                        app.open_project(ix, cx);
                    }
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .when(selected, |this| this.font_weight(FontWeight::MEDIUM))
                        .child(project.name.clone()),
                )
                .child(if self.is_analyzing(&project.path) {
                    // Analyse en cours : une petite icône qui tourne à la place de la note.
                    super::analysis_run::spinner(theme).into_any_element()
                } else {
                    GradeText::new(project.grade(), theme).into_any_element()
                })
                .when(!self.is_analyzing(&project.path), |this| {
                    this.child(
                        // Relancer l'analyse de ce projet, visible au survol de la ligne.
                        div()
                            .id(("rerun-project", ix))
                            .flex_none()
                            .p(metrics::SPACE_1)
                            .rounded(metrics::RADIUS)
                            .opacity(0.)
                            .group_hover(group.clone(), |style| style.opacity(1.))
                            .hover(|style| style.bg(theme.border_soft))
                            .on_click(cx.listener(move |app, _, _, cx| {
                                cx.stop_propagation();
                                app.analyze_project_at(ix, true, cx);
                            }))
                            .child(
                                Icon::new(IconName::Refresh, theme.ink_soft).size(metrics::ICON_SM),
                            ),
                    )
                })
                .child(
                    // Visible au survol de la ligne uniquement.
                    div()
                        .id(("remove-project", ix))
                        .flex_none()
                        .p(metrics::SPACE_1)
                        .rounded(metrics::RADIUS)
                        .opacity(0.)
                        .group_hover(group, |style| style.opacity(1.))
                        .hover(|style| style.bg(theme.border_soft))
                        .on_click(cx.listener(move |app, _, window, cx| {
                            cx.stop_propagation();
                            app.confirm_remove_project(ix, window, cx);
                        }))
                        .child(Icon::new(IconName::Close, theme.ink_soft).size(metrics::ICON_SM)),
                )
        });

        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(metrics::SPACE_2)
                    .pb(metrics::SPACE_1)
                    .child(section_label("Projets", theme))
                    .child(
                        div()
                            .id("add-project")
                            .p(metrics::SPACE_1)
                            .rounded(metrics::RADIUS)
                            .cursor_pointer()
                            .hover(|style| style.bg(theme.bg_hover))
                            .on_click(
                                cx.listener(|app, _, window, cx| app.open_new_project(window, cx)),
                            )
                            .child(
                                Icon::new(IconName::Plus, theme.ink_muted).size(metrics::ICON_SM),
                            ),
                    ),
            )
            .children(rows)
            .when(self.projects.is_empty(), |this| {
                this.child(
                    div()
                        .px(metrics::SPACE_2)
                        .text_size(metrics::TEXT_SM)
                        .text_color(theme.ink_soft)
                        .child("Aucun projet"),
                )
            })
    }

    /// Compte et quota d'analyses du jour (version officielle), mention « local ».
    fn render_plan(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let account_row = self.render_account_row(theme, cx);
        let quota = match self.usage() {
            Some(usage) => Some({
                let warning = match usage.remaining() {
                    Some(0) => Some("Quota atteint, il revient demain.".to_owned()),
                    Some(1) if usage.is_low() => Some("Il reste 1 analyse aujourd'hui.".to_owned()),
                    Some(n) if usage.is_low() => {
                        Some(format!("Il reste {n} analyses aujourd'hui."))
                    }
                    _ => None,
                };
                div()
                    .flex()
                    .flex_col()
                    .gap(metrics::SPACE_2)
                    .child(div().text_size(metrics::TEXT_SM).child(usage.label()))
                    // Pas de jauge sans limite (comptes admin).
                    .when(usage.limit.is_some(), |this| {
                        this.child(
                            ProgressBar::new(usage.ratio(), theme)
                                .when(usage.is_low(), |bar| bar.fill(theme.danger)),
                        )
                    })
                    .children(warning.map(|text| {
                        div()
                            .text_size(metrics::TEXT_SM)
                            .text_color(theme.danger)
                            .child(text)
                    }))
                    .into_any_element()
            }),
            None => self.quota_note().map(|note| {
                div()
                    .text_size(metrics::TEXT_SM)
                    .text_color(theme.ink_soft)
                    .child(note)
                    .into_any_element()
            }),
        };

        div()
            .flex()
            .flex_col()
            .gap(metrics::SPACE_3)
            .px(metrics::SPACE_2)
            .pt(metrics::SPACE_4)
            .border_t_1()
            .border_color(theme.border_soft)
            .children(quota)
            .children(account_row)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(metrics::SPACE_1)
                    .text_size(metrics::TEXT_XS)
                    .text_color(theme.ink_soft)
                    .child(Icon::new(IconName::Lock, theme.ink_soft).size(metrics::TEXT_SM))
                    .child("Local, sans IA"),
            )
    }
}

fn nav_item(
    id: &'static str,
    label: &'static str,
    active: bool,
    count: Option<usize>,
    theme: Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_between()
        .h(metrics::CONTROL_H)
        .px(metrics::SPACE_2)
        .rounded(metrics::RADIUS)
        .cursor_pointer()
        .when(active, |this| {
            this.bg(theme.bg_active).font_weight(FontWeight::MEDIUM)
        })
        .when(!active, |this| {
            this.text_color(theme.ink_muted)
                .hover(|style| style.bg(theme.bg_hover))
        })
        .on_click(on_click)
        .child(label)
        .children(count.map(|count| {
            div()
                .text_size(metrics::TEXT_SM)
                .font_weight(FontWeight::NORMAL)
                .text_color(theme.ink_soft)
                .child(count.to_string())
        }))
}

fn section_label(text: impl Into<SharedString>, theme: Theme) -> impl IntoElement {
    div()
        .text_size(metrics::TEXT_XS)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.ink_soft)
        .child(text.into())
}

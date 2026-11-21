//! Écran des alertes CVE : vulnérabilités publiées qui touchent les dépendances des projets.
//!
//! Filtres (projet, gravité, non lues) et pagination : la liste peut compter
//! des centaines d'alertes.

use std::path::PathBuf;

use gpui::{AnyElement, ElementId, FontWeight, SharedString, div, prelude::*, px};

use super::app::HeraldApp;
use super::components::{Button, ButtonVariant, SeverityLabel};
use super::home::{count_label, page_title, section_header, surface};
use crate::model::{Advisory, Severity};
use crate::theme::{Theme, fonts, metrics};

/// Alertes par page.
const ALERTS_PER_PAGE: usize = 20;

/// Filtres de l'écran des alertes et page affichée (gardés tant que l'app tourne).
#[derive(Default)]
pub(super) struct AlertFilter {
    project: Option<PathBuf>,
    severity: Option<Severity>,
    unread_only: bool,
    /// Page affichée, à partir de 0.
    page: usize,
}

impl AlertFilter {
    fn matches(&self, alert: &Advisory) -> bool {
        self.project
            .as_ref()
            .is_none_or(|path| alert.project_path == *path)
            && self
                .severity
                .is_none_or(|severity| alert.severity == severity)
            && (!self.unread_only || !alert.read)
    }
}

/// Nombre de pages pour `total` alertes (au moins une).
fn page_count(total: usize) -> usize {
    total.div_ceil(ALERTS_PER_PAGE).max(1)
}

const CONTENT_MAX_WIDTH: gpui::Pixels = px(960.);
const EXPLANATION_MAX_WIDTH: gpui::Pixels = px(640.);

impl HeraldApp {
    pub(super) fn render_alerts(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let unread = self.unread_alert_count();
        let filter = &self.alert_filter;
        let matching: Vec<usize> = self
            .alerts
            .iter()
            .enumerate()
            .filter(|(_, alert)| filter.matches(alert))
            .map(|(ix, _)| ix)
            .collect();
        let pages = page_count(matching.len());
        let page = filter.page.min(pages - 1);
        let rows = matching
            .iter()
            .skip(page * ALERTS_PER_PAGE)
            .take(ALERTS_PER_PAGE)
            .map(|&ix| alert_row(ix, &self.alerts[ix], theme, cx))
            .collect::<Vec<_>>();

        let status = format!(
            "{} · {}",
            count_label(matching.len() as u32, "alerte", "alertes"),
            match unread {
                0 => "aucune non lue".to_owned(),
                1 => "1 non lue".to_owned(),
                n => format!("{n} non lues"),
            }
        );

        div()
            .max_w(CONTENT_MAX_WIDTH)
            .flex()
            .flex_col()
            .gap(metrics::SPACE_5)
            .child(div().flex().child(page_title(
                "Alertes CVE",
                "Base OSV · dépendances de vos projets",
                theme,
            )))
            .child(
                div()
                    .max_w(EXPLANATION_MAX_WIDTH)
                    .text_color(theme.ink_muted)
                    .child(
                        "Herald compare les versions installées de vos dépendances, directes et \
                         transitives (lockfiles, sinon manifestes), à la base OSV, après chaque \
                         analyse et à chaque lancement. Seuls les noms et versions des paquets \
                         sont envoyés à osv.dev, jamais votre code.",
                    ),
            )
            .child(
                surface(theme)
                    .flex()
                    .flex_col()
                    .child(section_header("Vulnérabilités", status, theme).when(
                        unread > 0,
                        |this| {
                            this.child(
                                Button::new("mark-all-read", "Tout marquer comme lu", theme)
                                    .variant(ButtonVariant::Ghost)
                                    .on_click(
                                        cx.listener(|app, _, _, cx| app.mark_all_alerts_read(cx)),
                                    ),
                            )
                        },
                    ))
                    .when(!self.alerts.is_empty(), |this| {
                        this.child(self.alert_filters(theme, cx))
                    })
                    .when(pages > 1, |this| {
                        this.child(pager(
                            "alerts-pager-top",
                            page,
                            pages,
                            matching.len(),
                            theme,
                            cx,
                        ))
                    })
                    .children(rows)
                    .when(!self.alerts.is_empty() && matching.is_empty(), |this| {
                        this.child(
                            div()
                                .px(metrics::SPACE_4)
                                .py(metrics::SPACE_3)
                                .border_t_1()
                                .border_color(theme.border_soft)
                                .text_color(theme.ink_muted)
                                .child("Aucune alerte pour ces filtres."),
                        )
                    })
                    .when(pages > 1, |this| {
                        this.child(pager(
                            "alerts-pager-bottom",
                            page,
                            pages,
                            matching.len(),
                            theme,
                            cx,
                        ))
                    })
                    .when(self.alerts.is_empty(), |this| {
                        this.child(
                            div()
                                .px(metrics::SPACE_4)
                                .py(metrics::SPACE_3)
                                .border_t_1()
                                .border_color(theme.border_soft)
                                .text_color(theme.ink_muted)
                                .child("Aucune vulnérabilité connue dans vos dépendances."),
                        )
                    }),
            )
            .into_any_element()
    }

    /// Filtres : projet, gravité, non lues. Chaque bouton indique son nombre d'alertes.
    fn alert_filters(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let filter = &self.alert_filter;

        // Projets touchés, dans l'ordre de la barre latérale.
        let mut projects: Vec<(PathBuf, SharedString, usize)> = Vec::new();
        for alert in &self.alerts {
            match projects
                .iter_mut()
                .find(|(path, _, _)| *path == alert.project_path)
            {
                Some(entry) => entry.2 += 1,
                None => projects.push((alert.project_path.clone(), alert.project.clone(), 1)),
            }
        }
        let rank = |path: &PathBuf| {
            self.projects
                .iter()
                .position(|project| project.path == *path)
                .unwrap_or(usize::MAX)
        };
        projects.sort_by_key(|(path, _, _)| rank(path));

        let mut project_chips = vec![chip(
            "alerts-project-all",
            format!("Tous ({})", self.alerts.len()),
            filter.project.is_none(),
            theme,
            cx,
            |filter| filter.project = None,
        )];
        for (ix, (path, name, count)) in projects.into_iter().enumerate() {
            let active = filter.project.as_ref() == Some(&path);
            project_chips.push(chip(
                ("alerts-project", ix),
                format!("{name} ({count})"),
                active,
                theme,
                cx,
                move |filter| filter.project = Some(path.clone()),
            ));
        }

        let count_severity = |severity: Severity| {
            self.alerts
                .iter()
                .filter(|a| a.severity == severity)
                .count()
        };
        let mut severity_chips = vec![chip(
            "alerts-severity-all",
            "Toutes",
            filter.severity.is_none(),
            theme,
            cx,
            |filter| filter.severity = None,
        )];
        for (ix, severity) in [Severity::Critical, Severity::Warning, Severity::Info]
            .into_iter()
            .enumerate()
        {
            let count = count_severity(severity);
            if count == 0 {
                continue;
            }
            severity_chips.push(chip(
                ("alerts-severity", ix),
                format!("{} ({count})", severity.label()),
                filter.severity == Some(severity),
                theme,
                cx,
                move |filter| filter.severity = Some(severity),
            ));
        }
        let unread_only = filter.unread_only;
        severity_chips.push(div().w(metrics::SPACE_3).into_any_element());
        severity_chips.push(chip(
            "alerts-unread-only",
            "Non lues seulement",
            unread_only,
            theme,
            cx,
            move |filter| filter.unread_only = !unread_only,
        ));

        div()
            .flex()
            .flex_col()
            .gap(metrics::SPACE_2)
            .px(metrics::SPACE_4)
            .py(metrics::SPACE_3)
            .border_t_1()
            .border_color(theme.border_soft)
            .child(filter_row("Projet", project_chips, theme))
            .child(filter_row("Gravité", severity_chips, theme))
    }
}

/// Ligne de filtres : un intitulé, puis les boutons (retour à la ligne si besoin).
fn filter_row(label: &str, chips: Vec<AnyElement>, theme: Theme) -> impl IntoElement + use<> {
    div()
        .flex()
        .items_start()
        .gap(metrics::SPACE_3)
        .child(
            div()
                .flex_none()
                .w(px(56.))
                .pt(px(3.))
                .text_size(metrics::TEXT_SM)
                .text_color(theme.ink_muted)
                .child(label.to_owned()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_wrap()
                .gap(metrics::SPACE_2)
                .children(chips),
        )
}

/// Bouton de filtre ; un clic applique `apply` et revient à la première page.
fn chip(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
    apply: impl Fn(&mut AlertFilter) + 'static,
) -> AnyElement {
    div()
        .id(id.into())
        .px(metrics::SPACE_2)
        .py(px(3.))
        .rounded(metrics::RADIUS)
        .border_1()
        .text_size(metrics::TEXT_SM)
        .cursor_pointer()
        .when(active, |this| {
            this.bg(theme.bg_active)
                .border_color(theme.border)
                .font_weight(FontWeight::MEDIUM)
        })
        .when(!active, |this| {
            this.border_color(theme.border_soft)
                .text_color(theme.ink_muted)
                .hover(|style| style.bg(theme.bg_hover))
        })
        .on_click(cx.listener(move |app, _, _, cx| {
            apply(&mut app.alert_filter);
            app.alert_filter.page = 0;
            cx.notify();
        }))
        .child(label.into())
        .into_any_element()
}

/// « 21–40 sur 318 », puis Précédente / « Page 2 sur 16 » / Suivante.
fn pager(
    id: &'static str,
    page: usize,
    pages: usize,
    total: usize,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> impl IntoElement + use<> {
    let first = page * ALERTS_PER_PAGE + 1;
    let last = ((page + 1) * ALERTS_PER_PAGE).min(total);
    let step = |label: &'static str, target: Option<usize>, cx: &mut Context<HeraldApp>| {
        let enabled = target.is_some();
        div()
            .id(SharedString::from(format!("{id}-{label}")))
            .px(metrics::SPACE_2)
            .py(px(3.))
            .rounded(metrics::RADIUS)
            .text_size(metrics::TEXT_SM)
            .when(enabled, |this| {
                this.cursor_pointer()
                    .hover(|style| style.bg(theme.bg_hover))
                    .on_click(cx.listener(move |app, _, _, cx| {
                        if let Some(target) = target {
                            app.alert_filter.page = target;
                            cx.notify();
                        }
                    }))
            })
            .when(!enabled, |this| this.text_color(theme.ink_soft))
            .child(label)
    };
    div()
        .flex()
        .items_center()
        .gap(metrics::SPACE_2)
        .px(metrics::SPACE_4)
        .py(metrics::SPACE_2)
        .border_t_1()
        .border_color(theme.border_soft)
        .text_size(metrics::TEXT_SM)
        .child(
            div()
                .flex_1()
                .text_color(theme.ink_muted)
                .child(format!("{first}–{last} sur {total}")),
        )
        .child(step("‹ Précédente", page.checked_sub(1), cx))
        .child(
            div()
                .text_color(theme.ink_muted)
                .child(format!("Page {} sur {pages}", page + 1)),
        )
        .child(step(
            "Suivante ›",
            (page + 1 < pages).then_some(page + 1),
            cx,
        ))
}

fn alert_row(
    ix: usize,
    alert: &Advisory,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> impl IntoElement + use<> {
    let cvss = alert
        .cvss
        .map(|score| format!("CVSS {score:.1}").replace('.', ","));
    let published =
        crate::engine::cve::published_label(alert.published_unix, crate::engine::cve::unix_now());
    let url = alert.url.clone();
    let unread = !alert.read;

    div()
        .flex()
        .gap(metrics::SPACE_3)
        .px(metrics::SPACE_4)
        .py(metrics::SPACE_3)
        .border_t_1()
        .border_color(theme.border_soft)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(metrics::SPACE_1)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(metrics::SPACE_3)
                        .text_size(metrics::TEXT_SM)
                        .text_color(theme.ink_muted)
                        .child(SeverityLabel::new(alert.severity, theme))
                        .child(div().font_family(fonts::MONO).child(alert.id.clone()))
                        .children(cvss)
                        .child(div().flex_1())
                        .child(format!("publiée {published}")),
                )
                .child(
                    div()
                        .when(unread, |this| this.font_weight(FontWeight::SEMIBOLD))
                        .child(alert.summary.clone()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(metrics::SPACE_3)
                        .text_size(metrics::TEXT_SM)
                        .text_color(theme.ink_muted)
                        .child(
                            div()
                                .font_family(fonts::MONO)
                                .text_color(theme.ink)
                                .child(alert.upgrade_label()),
                        )
                        .child(format!(
                            "{} · {}{}{}",
                            alert.ecosystem,
                            alert.project,
                            if alert.direct { "" } else { " · transitive" },
                            if alert.confirmed {
                                ""
                            } else {
                                " · version déclarée, à confirmer"
                            }
                        ))
                        .child(div().flex_1())
                        .child(
                            Button::new(("open-alert-advisory", ix), "Voir l'avis", theme)
                                .variant(ButtonVariant::Ghost)
                                .on_click(move |_, _, cx| cx.open_url(&url)),
                        )
                        .child(
                            Button::new(("open-alert-project", ix), "Ouvrir le projet", theme)
                                .variant(ButtonVariant::Ghost)
                                .on_click(
                                    cx.listener(move |app, _, _, cx| {
                                        app.open_alert_project(ix, cx)
                                    }),
                                ),
                        )
                        .when(unread, |this| {
                            this.child(
                                Button::new(("mark-alert-read", ix), "Marquer comme lue", theme)
                                    .variant(ButtonVariant::Ghost)
                                    .on_click(cx.listener(move |app, _, _, cx| {
                                        app.mark_alert_read(ix, cx)
                                    })),
                            )
                        }),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::{ALERTS_PER_PAGE, page_count};

    #[test]
    fn pages_cover_every_alert() {
        assert_eq!(page_count(0), 1);
        assert_eq!(page_count(1), 1);
        assert_eq!(page_count(ALERTS_PER_PAGE), 1);
        assert_eq!(page_count(ALERTS_PER_PAGE + 1), 2);
        assert_eq!(page_count(318), 16);
    }
}

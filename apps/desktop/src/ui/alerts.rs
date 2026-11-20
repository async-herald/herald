//! Écran des alertes CVE : vulnérabilités publiées qui touchent les dépendances des projets.

use gpui::{AnyElement, FontWeight, div, prelude::*, px};

use super::app::HeraldApp;
use super::components::{Button, ButtonVariant, SeverityLabel};
use super::home::{page_title, section_header, surface};
use crate::model::Advisory;
use crate::theme::{Theme, fonts, metrics};

const CONTENT_MAX_WIDTH: gpui::Pixels = px(960.);
const EXPLANATION_MAX_WIDTH: gpui::Pixels = px(640.);

impl HeraldApp {
    pub(super) fn render_alerts(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let unread = self.unread_alert_count();
        let rows = self
            .alerts
            .iter()
            .enumerate()
            .map(|(ix, alert)| alert_row(ix, alert, theme, cx))
            .collect::<Vec<_>>();

        let status = match unread {
            0 => "Aucune nouvelle alerte".to_owned(),
            1 => "1 non lue".to_owned(),
            n => format!("{n} non lues"),
        };

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
                    .children(rows)
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

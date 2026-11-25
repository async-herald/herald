//! Écran « Règles » : catalogue des règles appliquées à chaque analyse, avec leur source.

use gpui::{AnyElement, FontWeight, SharedString, div, prelude::*, px};

use super::app::{HeraldApp, Screen};
use super::components::{Button, ButtonVariant, IconName, SeverityLabel};
use super::home::{count_label, page_title, surface};
use crate::model::{Category, Rule, RuleCatalog, RuleOrigin, rule_text, rule_title};
use crate::theme::{Theme, fonts, metrics};

const CONTENT_MAX_WIDTH: gpui::Pixels = px(960.);
const EXPLANATION_MAX_WIDTH: gpui::Pixels = px(640.);
const SEVERITY_WIDTH: gpui::Pixels = px(88.);
const CATEGORY_WIDTH: gpui::Pixels = px(96.);
const STATE_WIDTH: gpui::Pixels = px(132.);
const SWITCH_W: gpui::Pixels = px(30.);
const SWITCH_H: gpui::Pixels = px(18.);
const SWITCH_KNOB: gpui::Pixels = px(14.);
/// Règles officielles affichées par page.
const PAGE_SIZE: usize = 20;

impl HeraldApp {
    /// Ouvre l'écran et charge les règles la première fois.
    pub(super) fn open_rules(&mut self, cx: &mut Context<Self>) {
        self.screen = Screen::Rules;
        self.reload_custom_rules();
        if self.rules.is_none() {
            self.load_rules(cx);
        }
        cx.notify();
    }

    /// Recharge le catalogue (herald.codes, ou moteur hors ligne) sans bloquer l'interface.
    pub(super) fn load_rules(&mut self, cx: &mut Context<Self>) {
        if self.rules_loading {
            return;
        }
        self.rules_loading = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let catalog = cx
                .background_executor()
                .spawn(async { crate::engine::rules::catalog() })
                .await;
            this.update(cx, |app, cx| {
                app.rules = Some(catalog);
                app.rules_loading = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn set_rule_filter(&mut self, filter: Option<Category>, cx: &mut Context<Self>) {
        self.rule_filter = filter;
        self.custom_rules_tab = false;
        self.rule_page = 0;
        cx.notify();
    }

    fn set_rule_page(&mut self, page: usize, cx: &mut Context<Self>) {
        self.rule_page = page;
        cx.notify();
    }

    /// Active ou désactive une règle pour les prochaines analyses.
    fn set_rule_enabled(&mut self, id: SharedString, enabled: bool, cx: &mut Context<Self>) {
        let Some(catalog) = &mut self.rules else {
            return;
        };
        if let Some(choice) = catalog.set_enabled(&id, enabled) {
            crate::store::set_rule_override(&id, choice);
            cx.notify();
        }
    }

    /// Remet toutes les règles dans leur état par défaut.
    fn reset_rules(&mut self, cx: &mut Context<Self>) {
        if let Some(catalog) = &mut self.rules {
            catalog.reset();
        }
        crate::store::clear_rule_overrides();
        cx.notify();
    }

    pub(super) fn render_rules(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        if let Some(editor) = self.render_rule_editor(theme, cx) {
            return div()
                .max_w(CONTENT_MAX_WIDTH)
                .child(editor)
                .into_any_element();
        }
        let meta = match &self.rules {
            Some(catalog) => {
                let origin = match catalog.origin {
                    RuleOrigin::Online => "catalogue herald.codes",
                    RuleOrigin::Builtin => "hors ligne, catalogue intégré au moteur",
                };
                let mut meta = format!(
                    "{} actives sur {} · {origin}",
                    catalog.active_count(),
                    count_label(catalog.rules.len() as u32, "règle", "règles")
                );
                match catalog.overridden_count() {
                    0 => {}
                    1 => meta.push_str(" · 1 modifiée par vous"),
                    n => meta.push_str(&format!(" · {n} modifiées par vous")),
                }
                meta
            }
            None => "Chargement…".to_owned(),
        };

        let body: AnyElement = match &self.rules {
            Some(catalog) => self.render_rule_list(catalog, theme, cx).into_any_element(),
            None => surface(theme)
                .flex()
                .items_center()
                .gap(metrics::SPACE_2)
                .p(metrics::SPACE_5)
                .text_color(theme.ink_muted)
                .child(super::analysis_run::spinner(theme))
                .child("Chargement des règles…")
                .into_any_element(),
        };

        div()
            .max_w(CONTENT_MAX_WIDTH)
            .flex()
            .flex_col()
            .gap(metrics::SPACE_5)
            .child(
                div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .gap(metrics::SPACE_4)
                    .child(page_title("Règles", meta, theme))
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .gap(metrics::SPACE_2)
                            .when(
                                self.rules
                                    .as_ref()
                                    .is_some_and(|catalog| catalog.overridden_count() > 0),
                                |this| {
                                    this.child(
                                        Button::new("reset-rules", "Rétablir par défaut", theme)
                                            .variant(ButtonVariant::Ghost)
                                            .on_click(
                                                cx.listener(|app, _, _, cx| app.reset_rules(cx)),
                                            ),
                                    )
                                },
                            )
                            .child(
                                Button::new("reload-rules", "Actualiser", theme)
                                    .icon(IconName::Refresh)
                                    .on_click(cx.listener(|app, _, _, cx| app.load_rules(cx))),
                            )
                            .child(
                                Button::new("new-rule", "Nouvelle règle", theme)
                                    .variant(ButtonVariant::Primary)
                                    .icon(IconName::Plus)
                                    .on_click(cx.listener(|app, _, window, cx| {
                                        app.open_new_rule(window, cx)
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .max_w(EXPLANATION_MAX_WIDTH)
                    .text_color(theme.ink_muted)
                    .child(
                        "Chaque analyse applique ces règles sur votre machine. Activez ou \
                         désactivez une règle avec son interrupteur : le choix est gardé sur ce \
                         Mac et s'applique dès la prochaine analyse. Cliquez sur une règle pour \
                         lire son code.",
                    ),
            )
            .child(body)
            .into_any_element()
    }

    fn render_rule_list(
        &self,
        catalog: &RuleCatalog,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let all = (None, "Toutes", catalog.in_category(None).count());
        let by_category = Category::ALL.into_iter().filter_map(|category| {
            let count = catalog.in_category(Some(category)).count();
            (count > 0).then_some((Some(category), category.label(), count))
        });
        let tabs = std::iter::once(all)
            .chain(by_category)
            .map(|(filter, label, count)| {
                let active = !self.custom_rules_tab && self.rule_filter == filter;
                div()
                    .id(SharedString::from(format!("rule-filter-{label}")))
                    .flex()
                    .gap(metrics::SPACE_1)
                    .py(metrics::SPACE_2)
                    .border_b_2()
                    .cursor_pointer()
                    .when(active, |this| {
                        this.border_color(theme.accent)
                            .font_weight(FontWeight::MEDIUM)
                    })
                    .when(!active, |this| {
                        this.border_color(gpui::transparent_black())
                            .text_color(theme.ink_muted)
                            .hover(|style| style.text_color(theme.ink))
                    })
                    .on_click(cx.listener(move |app, _, _, cx| app.set_rule_filter(filter, cx)))
                    .child(label)
                    .child(div().text_color(theme.ink_soft).child(count.to_string()))
            })
            .collect::<Vec<_>>();

        let custom_active = self.custom_rules_tab;
        let custom_tab = div()
            .id("rule-filter-custom")
            .flex()
            .gap(metrics::SPACE_1)
            .py(metrics::SPACE_2)
            .border_b_2()
            .cursor_pointer()
            .when(custom_active, |this| {
                this.border_color(theme.accent)
                    .font_weight(FontWeight::MEDIUM)
            })
            .when(!custom_active, |this| {
                this.border_color(gpui::transparent_black())
                    .text_color(theme.ink_muted)
                    .hover(|style| style.text_color(theme.ink))
            })
            .on_click(cx.listener(|app, _, _, cx| app.show_custom_rules(cx)))
            .child("Mes règles")
            .child(
                div()
                    .text_color(theme.ink_soft)
                    .child(self.custom_rules.len().to_string()),
            );

        let total = catalog.in_category(self.rule_filter).count();
        let pages = total.div_ceil(PAGE_SIZE).max(1);
        // Le catalogue peut avoir rétréci depuis (Actualiser) : rester sur une page existante.
        let page = self.rule_page.min(pages - 1);
        let rows = if self.custom_rules_tab {
            self.render_custom_rule_list(theme, cx)
        } else {
            catalog
                .in_category(self.rule_filter)
                .skip(page * PAGE_SIZE)
                .take(PAGE_SIZE)
                .map(|rule| self.rule_row(rule, theme, cx))
                .collect::<Vec<_>>()
        };
        let first = page * PAGE_SIZE + 1;
        let last = (first + PAGE_SIZE - 1).min(total);

        let pagination = (pages > 1 && !self.custom_rules_tab).then(|| {
            div()
                .flex()
                .items_center()
                .gap(metrics::SPACE_2)
                .px(metrics::SPACE_4)
                .py(metrics::SPACE_2)
                .border_t_1()
                .border_color(theme.border_soft)
                .text_size(metrics::TEXT_SM)
                .text_color(theme.ink_muted)
                .child(div().flex_1().child(format!(
                    "{first}–{last} sur {total} · page {} sur {pages}",
                    page + 1
                )))
                .child(page_button(
                    "rules-prev",
                    "Précédent",
                    page.checked_sub(1),
                    theme,
                    cx,
                ))
                .child(page_button(
                    "rules-next",
                    "Suivant",
                    (page + 1 < pages).then_some(page + 1),
                    theme,
                    cx,
                ))
        });

        surface(theme)
            .flex_none()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_x(metrics::SPACE_4)
                    .px(metrics::SPACE_4)
                    .pt(metrics::SPACE_1)
                    .child(
                        div()
                            .flex()
                            .gap(metrics::SPACE_4)
                            .children(tabs)
                            .child(custom_tab),
                    ),
            )
            .children(rows)
            .children(pagination)
    }

    fn rule_row(&self, rule: &Rule, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let id = rule.id.clone();
        let modified = self.custom_rules.iter().any(|c| c.id == rule.id.as_ref());
        let state = if modified {
            Some("Version modifiée")
        } else {
            rule.is_overridden().then_some(if rule.enabled {
                "Activée par vous"
            } else {
                "Désactivée par vous"
            })
        };
        let toggle_id = rule.id.clone();
        let enabled = rule.enabled;
        let element_id = SharedString::from(format!("rule-{}", rule.id));

        let summary = div()
            .id(element_id)
            .flex()
            .items_center()
            .gap(metrics::SPACE_3)
            .px(metrics::SPACE_4)
            .py(metrics::SPACE_3)
            .cursor_pointer()
            .hover(|style| style.bg(theme.bg_hover))
            .on_click(cx.listener(move |app, _, window, cx| app.open_rule_page(&id, window, cx)))
            .child(
                div()
                    .w(SEVERITY_WIDTH)
                    .flex_none()
                    .child(SeverityLabel::new(rule.severity, theme)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(metrics::SPACE_1)
                    .when(!rule.enabled, |this| this.opacity(0.6))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(metrics::SPACE_2)
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(rule_title(&rule.id)),
                            )
                            .child(
                                div()
                                    .font_family(fonts::MONO)
                                    .text_size(metrics::TEXT_XS)
                                    .text_color(theme.ink_soft)
                                    .child(rule.id.clone()),
                            ),
                    )
                    // Une seule ligne : un texte replié change de hauteur selon la
                    // largeur, et gpui la calcule mal (lignes écrasées ou trop hautes).
                    // Le texte complet est dans la règle dépliée.
                    .map(|this| match rule_text(&rule.id) {
                        Some(text) => this.child(
                            div()
                                .truncate()
                                .text_size(metrics::TEXT_SM)
                                .text_color(theme.ink_muted)
                                .child(text.why),
                        ),
                        None => this.when(!rule.message.is_empty(), |this| {
                            this.child(
                                div()
                                    .truncate()
                                    .text_size(metrics::TEXT_SM)
                                    .text_color(theme.ink_muted)
                                    .child(rule.message.clone()),
                            )
                        }),
                    }),
            )
            .child(
                div()
                    .w(STATE_WIDTH)
                    .flex_none()
                    .flex()
                    .justify_end()
                    .text_size(metrics::TEXT_SM)
                    .text_color(theme.ink_soft)
                    .children(state),
            )
            .child(
                div()
                    .w(CATEGORY_WIDTH)
                    .flex_none()
                    .flex()
                    .justify_end()
                    .text_size(metrics::TEXT_SM)
                    .text_color(theme.ink_muted)
                    .child(rule.category.label()),
            )
            .child(rule_switch(
                SharedString::from(format!("rule-switch-{}", rule.id)),
                enabled,
                theme,
                cx.listener(move |app, _, _, cx| {
                    // Le clic ne doit pas aussi ouvrir la page de la règle.
                    cx.stop_propagation();
                    app.set_rule_enabled(toggle_id.clone(), !enabled, cx);
                }),
            ));

        div()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(theme.border_soft)
            .child(summary)
            .into_any_element()
    }
}

/// Bouton de pagination ; estompé et inactif sans page cible.
fn page_button(
    id: &'static str,
    label: &'static str,
    target: Option<usize>,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> impl IntoElement {
    let button = Button::new(id, label, theme).variant(ButtonVariant::Ghost);
    match target {
        Some(page) => div()
            .child(button.on_click(cx.listener(move |app, _, _, cx| app.set_rule_page(page, cx)))),
        None => div().opacity(0.4).child(button),
    }
}

/// Interrupteur activée / désactivée d'une règle.
pub(super) fn rule_switch(
    id: SharedString,
    on: bool,
    theme: Theme,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_none()
        .w(SWITCH_W)
        .h(SWITCH_H)
        .p(px(2.))
        .flex()
        .items_center()
        .rounded_full()
        .cursor_pointer()
        .bg(if on { theme.accent } else { theme.track })
        .when(on, |this| this.justify_end())
        .on_click(on_click)
        .child(
            div()
                .size(SWITCH_KNOB)
                .rounded_full()
                .bg(theme.bg_surface)
                .border_1()
                .border_color(if on { theme.accent } else { theme.border }),
        )
}

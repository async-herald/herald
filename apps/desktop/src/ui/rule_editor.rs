//! Page d'une règle, à la place de la liste de l'écran Règles : aperçu par
//! défaut (pourquoi, comment corriger, source, tests), « Modifier » pour
//! l'éditer. Une règle officielle modifiée devient votre version locale, qui
//! la remplace dans vos analyses ; les règles personnelles (onglet « Mes
//! règles ») se créent et se modifient de la même façon. Tout reste sur cette
//! machine.

use std::collections::HashSet;

use gpui::{
    AnyElement, Entity, Focusable, FontWeight, SharedString, Subscription, div, prelude::*, px,
};

use super::app::HeraldApp;
use super::components::{
    Button, ButtonVariant, CodeArea, CodeAreaEvent, Icon, IconName, SeverityLabel,
};
use super::home::surface;
use crate::engine::rule_check::{self, RuleCheck, TEMPLATE};
use crate::model::{rule_text, rule_title};
use crate::store::{self, CustomRule};
use crate::theme::{Theme, fonts, metrics};

/// Largeur du panneau de vérification : fixe, pour que ses textes se replient
/// à une largeur connue.
const CHECK_PANEL_WIDTH: gpui::Pixels = px(320.);
const CHECK_TEXT_WIDTH: gpui::Pixels = px(288.);
/// Colonne de l'aperçu : largeur fixe, même raison.
const PREVIEW_WIDTH: gpui::Pixels = px(600.);
const EDITOR_HEIGHT: gpui::Pixels = px(520.);

/// Quelle règle la page montre.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PageKind {
    New,
    /// Règle personnelle.
    Custom {
        id: String,
    },
    /// Règle officielle ; `overridden` : votre version la remplace.
    Official {
        id: String,
        editable: bool,
        overridden: bool,
    },
}

/// Page ouverte sur une règle.
pub(super) struct RuleEditor {
    kind: PageKind,
    /// Mode édition ; sinon aperçu.
    editing: bool,
    /// Source affichée en aperçu (et rétablie par « Annuler »).
    original: String,
    source: Entity<CodeArea>,
    check: RuleCheck,
    /// Identifiants pris par d'autres règles.
    taken: HashSet<String>,
    _subscription: Subscription,
}

impl HeraldApp {
    /// Relit les règles personnelles (à l'ouverture de l'écran Règles).
    pub(super) fn reload_custom_rules(&mut self) {
        self.custom_rules = store::custom_rules();
    }

    pub(super) fn show_custom_rules(&mut self, cx: &mut Context<Self>) {
        self.custom_rules_tab = true;
        cx.notify();
    }

    /// « Nouvelle règle » : la page s'ouvre en édition, sur le modèle.
    pub(super) fn open_new_rule(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        self.open_page(PageKind::New, TEMPLATE.to_owned(), true, window, cx);
    }

    /// Page d'une règle du catalogue ou de « Mes règles », en aperçu.
    pub(super) fn open_rule_page(
        &mut self,
        id: &str,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let custom = self.custom_rules.iter().find(|r| r.id == id).cloned();
        let official = self
            .rules
            .as_ref()
            .and_then(|catalog| catalog.rules.iter().find(|r| r.id.as_ref() == id))
            .cloned();
        let (kind, text) = match (official, custom) {
            (Some(official), custom) => (
                PageKind::Official {
                    id: id.to_owned(),
                    // Une règle écrite en Rust n'a pas de source `.herald`.
                    editable: herald_engine::dsl::parse_meta(&official.source).is_ok(),
                    overridden: custom.is_some(),
                },
                custom.map_or_else(|| official.source.to_string(), |c| c.source),
            ),
            (None, Some(custom)) => (PageKind::Custom { id: id.to_owned() }, custom.source),
            (None, None) => return,
        };
        self.open_page(kind, text, false, window, cx);
    }

    fn open_page(
        &mut self,
        kind: PageKind,
        text: String,
        editing: bool,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let own_id = match &kind {
            PageKind::New => None,
            PageKind::Custom { id } | PageKind::Official { id, .. } => Some(id.clone()),
        };
        let mut taken: HashSet<String> = match &self.rules {
            Some(catalog) => catalog.rules.iter().map(|r| r.id.to_string()).collect(),
            None => herald_engine::dsl::builtin_rules()
                .into_iter()
                .map(|r| r.id)
                .chain(
                    herald_engine::rules::extra_rules()
                        .into_iter()
                        .map(|r| r.id),
                )
                .collect(),
        };
        taken.extend(self.custom_rules.iter().map(|r| r.id.clone()));
        if let Some(id) = &own_id {
            taken.remove(id);
        }
        let check = rule_check::check(&text, |id| taken.contains(id));
        let (source, subscription) = self.source_area(&text, cx);
        if editing {
            window.focus(&source.read(cx).focus_handle(cx));
        }
        self.rule_editor = Some(RuleEditor {
            kind,
            editing,
            original: text,
            source,
            check,
            taken,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// Zone d'édition de la source ; chaque frappe revérifie la règle.
    fn source_area(
        &mut self,
        text: &str,
        cx: &mut Context<Self>,
    ) -> (Entity<CodeArea>, Subscription) {
        let source = cx.new(|cx| CodeArea::new(text, "regle.herald", cx));
        let subscription = cx.subscribe(&source, |app, source, event, cx| match event {
            CodeAreaEvent::Edited => {
                let text = source.read(cx).text().to_owned();
                if let Some(editor) = &mut app.rule_editor {
                    editor.check = rule_check::check(&text, |id| editor.taken.contains(id));
                    cx.notify();
                }
            }
            CodeAreaEvent::Save => app.save_rule_editor(cx),
            CodeAreaEvent::CursorMoved => {}
        });
        (source, subscription)
    }

    fn close_rule_editor(&mut self, cx: &mut Context<Self>) {
        self.rule_editor = None;
        cx.notify();
    }

    fn edit_rule(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        if let Some(editor) = &mut self.rule_editor {
            editor.editing = true;
            window.focus(&editor.source.read(cx).focus_handle(cx));
            cx.notify();
        }
    }

    /// « Annuler » : une nouvelle règle ferme la page ; sinon retour à l'aperçu,
    /// source d'origine rétablie.
    fn cancel_rule_edit(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &self.rule_editor else {
            return;
        };
        if editor.kind == PageKind::New {
            self.close_rule_editor(cx);
            return;
        }
        let original = editor.original.clone();
        self.reset_source(original, cx);
    }

    /// Remplace la source de la page et repasse en aperçu.
    fn reset_source(&mut self, text: String, cx: &mut Context<Self>) {
        let (source, subscription) = self.source_area(&text, cx);
        if let Some(editor) = &mut self.rule_editor {
            editor.check = rule_check::check(&text, |id| editor.taken.contains(id));
            editor.source = source;
            editor._subscription = subscription;
            editor.original = text;
            editor.editing = false;
        }
        cx.notify();
    }

    /// Enregistre la règle (si elle se lit et que son identifiant est libre),
    /// puis repasse en aperçu.
    fn save_rule_editor(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &self.rule_editor else {
            return;
        };
        if !editor.editing {
            return;
        }
        let Ok(summary) = &editor.check.summary else {
            return;
        };
        let id = summary.id.clone();
        let source = editor.source.read(cx).text().to_owned();
        let previous = match &editor.kind {
            PageKind::New => None,
            PageKind::Custom { id } => Some(id.clone()),
            PageKind::Official { id, overridden, .. } => overridden.then(|| id.clone()),
        };
        store::save_custom_rule(previous.as_deref(), &id, &source);
        self.reload_custom_rules();
        let is_official = self
            .rules
            .as_ref()
            .is_some_and(|catalog| catalog.rules.iter().any(|r| r.id.as_ref() == id));
        if let Some(editor) = &mut self.rule_editor {
            editor.kind = if is_official {
                PageKind::Official {
                    id: id.clone(),
                    editable: true,
                    overridden: true,
                }
            } else {
                PageKind::Custom { id: id.clone() }
            };
            editor.taken.remove(&id);
            editor.original = source;
            editor.editing = false;
        }
        cx.notify();
    }

    fn delete_rule_editor(&mut self, cx: &mut Context<Self>) {
        let Some(PageKind::Custom { id }) = self.rule_editor.as_ref().map(|e| e.kind.clone())
        else {
            return;
        };
        store::delete_custom_rule(&id);
        self.rule_editor = None;
        self.reload_custom_rules();
        cx.notify();
    }

    /// « Revenir à la version officielle » : votre version est supprimée.
    fn revert_to_official(&mut self, cx: &mut Context<Self>) {
        let Some(PageKind::Official { id, editable, .. }) =
            self.rule_editor.as_ref().map(|e| e.kind.clone())
        else {
            return;
        };
        store::delete_custom_rule(&id);
        self.reload_custom_rules();
        let official = self
            .rules
            .as_ref()
            .and_then(|catalog| catalog.rules.iter().find(|r| r.id.as_ref() == id))
            .map(|r| r.source.to_string())
            .unwrap_or_default();
        if let Some(editor) = &mut self.rule_editor {
            editor.kind = PageKind::Official {
                id,
                editable,
                overridden: false,
            };
        }
        self.reset_source(official, cx);
    }

    fn set_custom_rule_enabled(&mut self, id: String, enabled: bool, cx: &mut Context<Self>) {
        store::set_custom_rule_enabled(&id, enabled);
        self.reload_custom_rules();
        cx.notify();
    }

    /// Liste « Mes règles » (dans la surface de l'écran Règles).
    pub(super) fn render_custom_rule_list(
        &self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if self.custom_rules.is_empty() {
            return vec![
                div()
                    .px(metrics::SPACE_4)
                    .py(metrics::SPACE_5)
                    .border_t_1()
                    .border_color(theme.border_soft)
                    .flex()
                    .flex_col()
                    .gap(metrics::SPACE_1)
                    .child(div().font_weight(FontWeight::MEDIUM).child("Aucune règle personnelle."))
                    .child(
                        div()
                            .text_size(metrics::TEXT_SM)
                            .text_color(theme.ink_muted)
                            .child("« Nouvelle règle » pour en écrire une : elle s'applique à vos analyses, sur cette machine."),
                    )
                    .into_any_element(),
            ];
        }
        let official: HashSet<&str> = self
            .rules
            .iter()
            .flat_map(|catalog| catalog.rules.iter().map(|r| r.id.as_ref()))
            .collect();
        self.custom_rules
            .iter()
            .map(|rule| custom_rule_row(rule, official.contains(rule.id.as_str()), theme, cx))
            .collect()
    }

    /// Page de la règle, à la place de la liste des règles.
    pub(super) fn render_rule_editor(
        &self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let editor = self.rule_editor.as_ref()?;
        let editing = editor.editing;
        let can_save = editor.check.can_save();

        let (title, id, badge, note): (String, Option<String>, Option<&str>, &str) = match &editor
            .kind
        {
            PageKind::New => (
                "Nouvelle règle".to_owned(),
                None,
                None,
                "Langage .herald · gardée sur cette machine · appliquée dès la prochaine analyse",
            ),
            PageKind::Custom { id } => (
                rule_title(id),
                Some(id.clone()),
                Some("Personnelle"),
                "Gardée sur cette machine · appliquée à vos analyses",
            ),
            PageKind::Official {
                id,
                editable,
                overridden,
            } => (
                rule_title(id),
                Some(id.clone()),
                Some(if *overridden {
                    "Version modifiée par vous"
                } else {
                    "Officielle"
                }),
                if !editable {
                    "Règle écrite en Rust dans le moteur : pas de source .herald à modifier"
                } else if *overridden {
                    "Votre version remplace la version officielle dans vos analyses, sur cette machine"
                } else {
                    "« Modifier » crée votre version, qui remplacera l'officielle dans vos analyses"
                },
            ),
        };

        let buttons = div().flex().flex_none().gap(metrics::SPACE_2).map(|this| {
            if editing {
                this.child(
                    Button::new("rule-page-cancel", "Annuler", theme)
                        .on_click(cx.listener(|app, _, _, cx| app.cancel_rule_edit(cx))),
                )
                .child(
                    div().when(!can_save, |this| this.opacity(0.5)).child(
                        Button::new("rule-page-save", "Enregistrer", theme)
                            .variant(ButtonVariant::Primary)
                            .icon(IconName::Check)
                            .on_click(cx.listener(|app, _, _, cx| app.save_rule_editor(cx))),
                    ),
                )
            } else {
                let editable = !matches!(
                    editor.kind,
                    PageKind::Official {
                        editable: false,
                        ..
                    }
                );
                this.when(matches!(editor.kind, PageKind::Custom { .. }), |this| {
                    this.child(
                        Button::new("rule-page-delete", "Supprimer", theme)
                            .variant(ButtonVariant::Ghost)
                            .on_click(cx.listener(|app, _, _, cx| app.delete_rule_editor(cx))),
                    )
                })
                .when(
                    matches!(
                        editor.kind,
                        PageKind::Official {
                            overridden: true,
                            ..
                        }
                    ),
                    |this| {
                        this.child(
                            Button::new(
                                "rule-page-revert",
                                "Revenir à la version officielle",
                                theme,
                            )
                            .variant(ButtonVariant::Ghost)
                            .on_click(cx.listener(|app, _, _, cx| app.revert_to_official(cx))),
                        )
                    },
                )
                .when(editable, |this| {
                    this.child(
                        Button::new("rule-page-edit", "Modifier", theme)
                            .variant(ButtonVariant::Primary)
                            .icon(IconName::Pencil)
                            .on_click(cx.listener(|app, _, window, cx| app.edit_rule(window, cx))),
                    )
                })
            }
        });

        let header = div()
            .flex()
            .items_end()
            .justify_between()
            .gap(metrics::SPACE_4)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(metrics::SPACE_1)
                    .child(
                        div()
                            .id("rule-page-back")
                            .flex()
                            .items_center()
                            .gap(metrics::SPACE_1)
                            .text_size(metrics::TEXT_SM)
                            .text_color(theme.ink_muted)
                            .cursor_pointer()
                            .hover(|style| style.text_color(theme.ink))
                            .on_click(cx.listener(|app, _, _, cx| app.close_rule_editor(cx)))
                            .child(
                                Icon::new(IconName::ChevronLeft, theme.ink_muted)
                                    .size(metrics::ICON_SM),
                            )
                            .child("Règles"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(metrics::SPACE_2)
                            .child(
                                div()
                                    .text_size(metrics::TEXT_XL)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .children(badge.map(|badge| {
                                div()
                                    .px(metrics::SPACE_2)
                                    .rounded_full()
                                    .bg(theme.bg_active)
                                    .text_size(metrics::TEXT_XS)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.ink_muted)
                                    .child(badge)
                            })),
                    )
                    .children(id.map(|id| {
                        div()
                            .font_family(fonts::MONO)
                            .text_size(metrics::TEXT_SM)
                            .text_color(theme.ink_soft)
                            .child(id)
                    }))
                    .child(
                        div()
                            .text_size(metrics::TEXT_SM)
                            .text_color(theme.ink_muted)
                            .child(note),
                    ),
            )
            .child(buttons);

        let main: AnyElement = if editing {
            div()
                .flex_1()
                .min_w_0()
                .h(EDITOR_HEIGHT)
                .rounded(metrics::RADIUS_LG)
                .overflow_hidden()
                .border_1()
                .border_color(theme.border)
                .child(editor.source.clone())
                .into_any_element()
        } else {
            preview(&editor.kind, &editor.original, theme).into_any_element()
        };

        Some(
            div()
                .flex()
                .flex_col()
                .gap(metrics::SPACE_5)
                .child(header)
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(metrics::SPACE_4)
                        .child(main)
                        .child(check_panel(&editor.check, theme)),
                )
                .into_any_element(),
        )
    }
}

/// Aperçu : pourquoi, comment corriger (règles officielles), puis la source.
fn preview(kind: &PageKind, source: &str, theme: Theme) -> impl IntoElement {
    let text = match kind {
        PageKind::Official { id, .. } | PageKind::Custom { id } => rule_text(id),
        PageKind::New => None,
    };
    let paragraph = |content: String| {
        div()
            .w(PREVIEW_WIDTH)
            .text_size(metrics::TEXT_BASE)
            .child(content)
    };
    let lines = source
        .lines()
        .map(|line| {
            div()
                .whitespace_nowrap()
                .child(SharedString::from(line.replace('\t', "    ")))
        })
        .collect::<Vec<_>>();
    div()
        .w(PREVIEW_WIDTH)
        .flex_none()
        .flex()
        .flex_col()
        .gap(metrics::SPACE_4)
        .children(text.map(|text| {
            div()
                .flex()
                .flex_col()
                .gap(metrics::SPACE_2)
                .child(caption("Pourquoi", theme))
                .child(paragraph(text.why.to_string()))
                .child(div().h(metrics::SPACE_1))
                .child(caption("Comment corriger", theme))
                .child(paragraph(text.fix.to_string()))
        }))
        .child(caption("Source", theme))
        .child(
            div()
                .id("rule-page-source")
                .w(PREVIEW_WIDTH)
                .overflow_x_scroll()
                .rounded(metrics::RADIUS)
                .bg(theme.code_bg)
                .border_1()
                .border_color(theme.border_soft)
                .px(metrics::SPACE_3)
                .py(metrics::SPACE_2)
                .font_family(fonts::MONO)
                .text_size(metrics::TEXT_SM)
                .line_height(metrics::SPACE_5)
                .text_color(theme.code_text)
                .children(lines),
        )
}

/// Une règle personnelle dans « Mes règles » : un clic ouvre sa page.
/// `replaces_official` : votre version d'une règle officielle.
fn custom_rule_row(
    rule: &CustomRule,
    replaces_official: bool,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> AnyElement {
    let meta = herald_engine::dsl::parse_meta(&rule.source).ok();
    let severity = meta.as_ref().map_or(crate::model::Severity::Info, |m| {
        crate::model::Severity::from_engine(&m.severity)
    });
    let message = meta.as_ref().map(|m| m.message.clone()).unwrap_or_default();
    let category = meta
        .as_ref()
        .and_then(|m| crate::model::Category::from_engine(&m.category))
        .map_or("—", |c| c.label());
    let enabled = rule.enabled;
    let open = rule.id.clone();
    let toggle_id = rule.id.clone();

    div()
        .id(SharedString::from(format!("custom-rule-{}", rule.id)))
        .flex()
        .items_center()
        .gap(metrics::SPACE_3)
        .px(metrics::SPACE_4)
        .py(metrics::SPACE_3)
        .border_t_1()
        .border_color(theme.border_soft)
        .cursor_pointer()
        .hover(|style| style.bg(theme.bg_hover))
        .on_click(cx.listener(move |app, _, window, cx| app.open_rule_page(&open, window, cx)))
        .child(
            div()
                .w(px(88.))
                .flex_none()
                .child(SeverityLabel::new(severity, theme)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(metrics::SPACE_1)
                .when(!enabled, |this| this.opacity(0.6))
                .child(
                    div()
                        .font_family(fonts::MONO)
                        .text_size(metrics::TEXT_SM)
                        .font_weight(FontWeight::MEDIUM)
                        .child(rule.id.clone()),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(metrics::TEXT_SM)
                        .text_color(theme.ink_muted)
                        .child(message),
                ),
        )
        .child(
            div()
                .flex_none()
                .text_size(metrics::TEXT_SM)
                .text_color(theme.ink_soft)
                .child(if replaces_official {
                    "Remplace l'officielle"
                } else {
                    "Personnelle"
                }),
        )
        .child(
            div()
                .w(px(96.))
                .flex_none()
                .flex()
                .justify_end()
                .text_size(metrics::TEXT_SM)
                .text_color(theme.ink_muted)
                .child(category),
        )
        .child(super::rules::rule_switch(
            SharedString::from(format!("custom-rule-switch-{}", rule.id)),
            enabled,
            theme,
            cx.listener(move |app, _, _, cx| {
                cx.stop_propagation();
                app.set_custom_rule_enabled(toggle_id.clone(), !enabled, cx);
            }),
        ))
        .into_any_element()
}

/// Panneau de droite : la règle se lit-elle, et ses tests passent-ils ?
fn check_panel(check: &RuleCheck, theme: Theme) -> impl IntoElement {
    let status: AnyElement = match &check.summary {
        Ok(summary) => div()
            .flex()
            .flex_col()
            .gap(metrics::SPACE_1)
            .p(metrics::SPACE_3)
            .rounded(metrics::RADIUS)
            .bg(theme.success_bg)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(metrics::SPACE_2)
                    .text_color(theme.success)
                    .font_weight(FontWeight::MEDIUM)
                    .child(Icon::new(IconName::Check, theme.success).size(metrics::ICON_SM))
                    .child("La règle se lit"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(metrics::SPACE_2)
                    .text_size(metrics::TEXT_SM)
                    .child(SeverityLabel::new(summary.severity, theme))
                    .child(
                        div()
                            .text_color(theme.ink_muted)
                            .child(summary.category.map_or("catégorie inconnue", |c| c.label())),
                    ),
            )
            .child(
                div()
                    .w(CHECK_TEXT_WIDTH)
                    .text_size(metrics::TEXT_SM)
                    .text_color(theme.ink_muted)
                    .child(summary.message.clone()),
            )
            .into_any_element(),
        Err(message) => div()
            .flex()
            .flex_col()
            .gap(metrics::SPACE_1)
            .p(metrics::SPACE_3)
            .rounded(metrics::RADIUS)
            .bg(theme.code_line_critical)
            .child(
                div()
                    .text_color(theme.danger)
                    .font_weight(FontWeight::MEDIUM)
                    .child("À corriger avant d'enregistrer"),
            )
            .child(
                div()
                    .w(CHECK_TEXT_WIDTH)
                    .text_size(metrics::TEXT_SM)
                    .child(message.clone()),
            )
            .into_any_element(),
    };

    let total = check.tests.len();
    let failed = check.failed_tests();
    let tests_title = match (total, failed) {
        (0, _) => "Aucun test".to_owned(),
        (_, 0) => format!(
            "{total} test{} sur {total} {}",
            plural(total),
            verb(total, "passe", "passent")
        ),
        _ => format!(
            "{failed} test{} sur {total} {}",
            plural(failed),
            verb(failed, "échoue", "échouent")
        ),
    };

    div()
        .w(CHECK_PANEL_WIDTH)
        .flex_none()
        .child(
            surface(theme)
                .flex()
                .flex_col()
                .gap(metrics::SPACE_3)
                .p(metrics::SPACE_4)
                .child(caption("Vérification", theme))
                .child(status)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(caption("Tests", theme))
                        .child(
                            div()
                                .text_size(metrics::TEXT_SM)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(if failed > 0 {
                                    theme.danger
                                } else if total > 0 {
                                    theme.success
                                } else {
                                    theme.ink_muted
                                })
                                .child(tests_title),
                        ),
                )
                .when(total == 0, |this| {
                    this.child(
                        div()
                            .w(CHECK_TEXT_WIDTH)
                            .text_size(metrics::TEXT_SM)
                            .text_color(theme.ink_muted)
                            .child("Ajoutez un bloc tests : « => fail » pour du code à relever, « => pass » pour du code à laisser passer."),
                    )
                })
                .children(check.tests.iter().map(|test| {
                    let (icon, color) = if test.passed {
                        (IconName::Check, theme.success)
                    } else {
                        (IconName::Close, theme.danger)
                    };
                    div()
                        .flex()
                        .items_start()
                        .gap(metrics::SPACE_2)
                        .child(div().mt(px(2.)).child(Icon::new(icon, color).size(metrics::ICON_SM)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .truncate()
                                        .font_family(fonts::MONO)
                                        .text_size(metrics::TEXT_XS)
                                        .child(test.code.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(metrics::TEXT_XS)
                                        .text_color(if test.passed { theme.ink_soft } else { theme.danger })
                                        .child(format!(
                                            "{} · {}",
                                            test.target,
                                            match (test.expect_flag, test.passed) {
                                                (true, true) => "relevé, comme attendu",
                                                (false, true) => "laissé passer, comme attendu",
                                                (true, false) => "devait être relevé",
                                                (false, false) => "relevé à tort",
                                            }
                                        )),
                                ),
                        )
                })),
        )
}

fn caption(text: &str, theme: Theme) -> impl IntoElement {
    div()
        .text_size(metrics::TEXT_XS)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.ink_soft)
        .child(text.to_uppercase())
}

fn plural(n: usize) -> &'static str {
    if n > 1 { "s" } else { "" }
}

fn verb(n: usize, singular: &'static str, plural: &'static str) -> &'static str {
    if n > 1 { plural } else { singular }
}

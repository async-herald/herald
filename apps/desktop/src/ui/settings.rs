//! Écran « Réglages » : compte (version officielle), analyse, données locales
//! et à propos.

use gpui::{
    AnyElement, FontWeight, PromptButton, PromptLevel, SharedString, Window, div, prelude::*, px,
};

use super::app::{HeraldApp, Screen};
use super::components::{Button, ButtonVariant, IconName};
use super::home::{page_title, surface};
use super::rules::rule_switch;
use crate::store;
use crate::theme::{Theme, ThemeMode, metrics};

/// Réglage local : revérifier les CVE des projets au lancement.
pub(super) const CVE_ON_START: &str = "cve_on_start";

const CONTENT_MAX_WIDTH: gpui::Pixels = px(760.);
const TEXT_MAX_WIDTH: gpui::Pixels = px(380.);

/// Onglets de l'écran Réglages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum SettingsTab {
    /// Compte herald.codes (version officielle seulement).
    #[default]
    Account,
    Appearance,
    Analysis,
    /// Faux positifs signalés depuis ce compte (version officielle seulement).
    Reports,
    Data,
    About,
}

#[derive(Default)]
pub(super) struct SettingsState {
    pub(super) tab: SettingsTab,
    pub(super) busy: bool,
    /// Dernier message : texte et `true` pour une erreur.
    pub(super) message: Option<(SharedString, bool)>,
}

impl HeraldApp {
    pub(super) fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.screen = Screen::Settings;
        self.settings.message = None;
        self.prepare_account_settings(cx);
        cx.notify();
    }

    /// Demande confirmation avant une action irréversible.
    pub(super) fn confirm(
        &mut self,
        title: &str,
        detail: &str,
        button: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
        on_confirm: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let answer = window.prompt(
            PromptLevel::Warning,
            title,
            Some(detail),
            &[PromptButton::ok(button), PromptButton::cancel("Annuler")],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(0) {
                this.update_in(cx, |app, window, cx| on_confirm(app, window, cx))
                    .ok();
            }
        })
        .detach();
    }

    /// Efface projets, analyses, alertes et cache sur cette machine.
    fn clear_local_data(&mut self, cx: &mut Context<Self>) {
        store::clear_local_data();
        let paths: Vec<_> = self
            .projects
            .iter()
            .map(|project| project.path.clone())
            .collect();
        for path in paths {
            self.forget_ide(&path);
        }
        self.projects.clear();
        self.alerts.clear();
        self.selected_project = 0;
        self.rules = None;
        self.reset_selection();
        self.settings.message = Some(("Données locales effacées.".into(), false));
        cx.notify();
    }

    pub(super) fn render_settings(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let busy = self.settings.busy;
        let cve_on_start = store::setting(CVE_ON_START, true);
        let data_dir = store::data_dir()
            .map(|dir| dir.display().to_string())
            .unwrap_or_default();
        let size = store::database_size();

        let account_section = self.render_account_section(theme, cx);
        let has_account = account_section.is_some();
        // Version locale : ni Compte ni Signalements, on ouvre Apparence.
        let tab = match self.settings.tab {
            SettingsTab::Account | SettingsTab::Reports if !has_account => SettingsTab::Appearance,
            tab => tab,
        };

        let mode = self.theme_mode;
        let appearance_section = section("Apparence", theme).child(row(
            "Thème",
            "Clair, sombre, ou selon l'apparence de macOS.",
            theme,
            div()
                .flex()
                .p(px(2.))
                .gap(px(2.))
                .rounded(metrics::RADIUS)
                .bg(theme.bg_hover)
                .border_1()
                .border_color(theme.border)
                .children(ThemeMode::ALL.into_iter().map(|choice| {
                    let on = choice == mode;
                    div()
                        .id(choice.key())
                        .px(metrics::SPACE_3)
                        .py(px(4.))
                        .rounded(metrics::RADIUS)
                        .text_size(metrics::TEXT_SM)
                        .cursor_pointer()
                        .when(on, |this| {
                            this.bg(theme.bg_surface)
                                .font_weight(FontWeight::SEMIBOLD)
                                .border_1()
                                .border_color(theme.border)
                        })
                        .when(!on, |this| {
                            this.text_color(theme.ink_muted)
                                .hover(|style| style.text_color(theme.ink))
                        })
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.theme_mode = choice;
                            store::set_text_setting(super::app::THEME_MODE, choice.key());
                            cx.notify();
                        }))
                        .child(choice.label())
                })),
        ));

        let analysis_section = section("Analyse", theme)
            .child(row(
                "Vérifier les nouvelles CVE au lancement",
                "Compare les dépendances de vos projets à la base OSV à chaque ouverture de l'app. \
                 Seuls les noms et versions des paquets sont envoyés.",
                theme,
                rule_switch(
                    "settings-cve-on-start".into(),
                    cve_on_start,
                    theme,
                    cx.listener(move |_, _, _, cx| {
                        store::set_setting(CVE_ON_START, !cve_on_start);
                        cx.notify();
                    }),
                ),
            ))
            .child(row(
                "Fichiers analysés au maximum",
                "Au-delà, les fichiers sont pris à tour de rôle dans chaque dossier du projet. \
                 Une limite haute demande plus de mémoire.",
                theme,
                file_limit_switch(crate::engine::max_files(), theme, cx),
            ))
            .child(row(
                "Règles appliquées",
                "Activez ou désactivez les règles utilisées par l'analyse.",
                theme,
                Button::new("settings-open-rules", "Ouvrir les règles", theme)
                    .on_click(cx.listener(|app, _, _, cx| app.open_rules(cx))),
            ));

        let data_section = section("Données sur ce Mac", theme)
            .child(row(
                "Emplacement",
                "Projets, rapports, alertes et cache. Le code de vos projets n'y est pas copié.",
                theme,
                div()
                    .flex()
                    .items_center()
                    .gap(metrics::SPACE_2)
                    .child(
                        div()
                            .text_size(metrics::TEXT_SM)
                            .text_color(theme.ink_muted)
                            .child(format_size(size)),
                    )
                    .child({
                        let dir = data_dir.clone();
                        Button::new("settings-reveal", "Ouvrir dans le Finder", theme)
                            .on_click(move |_, _, cx| cx.open_url(&format!("file://{dir}")))
                    }),
            ))
            .child(row(
                "Effacer les données locales",
                "Retire tous les projets de l'app, avec leurs rapports et alertes. Les dossiers \
                 de vos projets ne sont pas touchés.",
                theme,
                Button::new("settings-clear-local", "Effacer", theme)
                    .variant(ButtonVariant::Ghost)
                    .on_click(cx.listener(|app, _, window, cx| {
                        app.confirm(
                            "Effacer les données locales ?",
                            "Projets, rapports, alertes et choix de règles seront retirés de l'app. \
                             Vos dossiers restent sur le disque.",
                            "Effacer",
                            window,
                            cx,
                            |app, _, cx| app.clear_local_data(cx),
                        );
                    })),
            ));

        let about_section = section("À propos", theme).child(row(
            &format!("Herald Desktop {}", env!("CARGO_PKG_VERSION")),
            "Analyse 100 % locale, sans IA. Seuls le nom du projet et la note sont envoyés à votre compte.",
            theme,
            Button::new("settings-source", "Code du moteur", theme)
                .variant(ButtonVariant::Ghost)
                .icon(IconName::GitHub)
                .on_click(|_, _, cx| cx.open_url("https://github.com/async-herald/herald")),
        ));

        div()
            .max_w(CONTENT_MAX_WIDTH)
            .flex()
            .flex_col()
            .gap(metrics::SPACE_5)
            .child(div().flex_none().child(page_title(
                "Réglages",
                if busy {
                    "Enregistrement…"
                } else if has_account {
                    "Compte, apparence, analyse et données locales"
                } else {
                    "Apparence, analyse et données locales"
                },
                theme,
            )))
            .children(self.settings.message.clone().map(|(text, error)| {
                surface(theme)
                    .flex_none()
                    .px(metrics::SPACE_4)
                    .py(metrics::SPACE_3)
                    .text_color(if error { theme.danger } else { theme.success })
                    .child(text)
            }))
            .child(
                div()
                    .flex_none()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(settings_tabs(tab, has_account, theme, cx)),
            )
            .child(match tab {
                SettingsTab::Account => account_section.unwrap_or_else(|| div().into_any_element()),
                SettingsTab::Appearance => appearance_section.into_any_element(),
                SettingsTab::Analysis => analysis_section.into_any_element(),
                SettingsTab::Reports => self
                    .render_reports_section(theme, cx)
                    .unwrap_or_else(|| div().into_any_element()),
                SettingsTab::Data => data_section.into_any_element(),
                SettingsTab::About => about_section.into_any_element(),
            })
            .into_any_element()
    }
}

/// Choix de la limite de fichiers, au style du choix du thème.
fn file_limit_switch(
    current: usize,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> impl IntoElement {
    use crate::engine::collect::{FILE_LIMITS, MAX_FILES_SETTING};
    div()
        .flex()
        .p(px(2.))
        .gap(px(2.))
        .rounded(metrics::RADIUS)
        .bg(theme.bg_hover)
        .border_1()
        .border_color(theme.border)
        .children(FILE_LIMITS.into_iter().map(|limit| {
            let on = limit == current;
            div()
                .id(("file-limit", limit))
                .px(metrics::SPACE_3)
                .py(px(4.))
                .rounded(metrics::RADIUS)
                .text_size(metrics::TEXT_SM)
                .cursor_pointer()
                .when(on, |this| {
                    this.bg(theme.bg_surface)
                        .font_weight(FontWeight::SEMIBOLD)
                        .border_1()
                        .border_color(theme.border)
                })
                .when(!on, |this| {
                    this.text_color(theme.ink_muted)
                        .hover(|style| style.text_color(theme.ink))
                })
                .on_click(cx.listener(move |_, _, _, cx| {
                    store::set_text_setting(MAX_FILES_SETTING, &limit.to_string());
                    cx.notify();
                }))
                .child(super::home::thousands(limit as u32))
        }))
}

/// Barre d'onglets des réglages, au style de celle de l'écran Problèmes.
fn settings_tabs(
    current: SettingsTab,
    has_account: bool,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> impl IntoElement {
    let tab = |id: &'static str, label: &'static str, value: SettingsTab| {
        super::analysis::underline_tab(
            id,
            label,
            current == value,
            theme,
            cx.listener(move |app, _, window, cx| {
                app.settings.tab = value;
                // Liste relue à chaque ouverture de l'onglet : un statut a pu changer.
                if value == SettingsTab::Reports {
                    app.load_own_reports(window, cx);
                }
                cx.notify();
            }),
        )
    };
    div()
        .flex()
        .gap(metrics::SPACE_6)
        .when(has_account, |this| {
            this.child(tab("settings-tab-account", "Compte", SettingsTab::Account))
        })
        .child(tab(
            "settings-tab-appearance",
            "Apparence",
            SettingsTab::Appearance,
        ))
        .child(tab(
            "settings-tab-analysis",
            "Analyse",
            SettingsTab::Analysis,
        ))
        .when(has_account, |this| {
            this.child(tab(
                "settings-tab-reports",
                "Signalements",
                SettingsTab::Reports,
            ))
        })
        .child(tab("settings-tab-data", "Données", SettingsTab::Data))
        .child(tab("settings-tab-about", "À propos", SettingsTab::About))
}

/// Bloc de réglages avec son titre.
pub(super) fn section(title: &'static str, theme: Theme) -> gpui::Div {
    surface(theme).flex_none().flex().flex_col().child(
        div()
            .px(metrics::SPACE_4)
            .pt(metrics::SPACE_4)
            .pb(metrics::SPACE_2)
            .text_size(metrics::TEXT_XS)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.ink_soft)
            .child(title.to_uppercase()),
    )
}

/// Une ligne : intitulé et explication à gauche, contrôle à droite.
pub(super) fn row(
    label: &str,
    detail: &str,
    theme: Theme,
    control: impl IntoElement,
) -> impl IntoElement {
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(metrics::SPACE_4)
        .px(metrics::SPACE_4)
        .py(metrics::SPACE_3)
        .border_t_1()
        .border_color(theme.border_soft)
        .child(
            div()
                // Largeur fixe : GPUI mesure alors le texte à sa vraie largeur
                // et la ligne prend la hauteur du texte replié.
                .w(TEXT_MAX_WIDTH)
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child(label.to_owned()),
                )
                .child(
                    div()
                        .text_size(metrics::TEXT_SM)
                        .text_color(theme.ink_muted)
                        .child(detail.to_owned()),
                ),
        )
        .child(div().flex_1())
        .child(div().flex_none().child(control))
}

/// « 12,4 Mo ».
fn format_size(bytes: u64) -> String {
    let mb = bytes as f64 / 1_048_576.;
    if mb >= 1. {
        format!("{mb:.1} Mo").replace('.', ",")
    } else {
        format!("{} Ko", bytes / 1024)
    }
}

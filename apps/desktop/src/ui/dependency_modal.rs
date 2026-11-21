//! Modale d'un paquet (onglet Dépendances) : le problème (version, avis OSV,
//! version corrigée), la commande qui le corrige, et « Corriger », qui exécute
//! cette commande dans le dossier du projet en montrant sa sortie. Une fois
//! la correction faite, « Relancer l'analyse » relit les versions installées.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationExt, AnyElement, ClipboardItem, FocusHandle, FontWeight, KeyDownEvent,
    SharedString, Task, div, prelude::*, px,
};

use super::app::HeraldApp;
use super::components::{Button, ButtonVariant, Icon, IconName, SeverityLabel};
use super::home::count_label;
use crate::engine::cve::{self, Dependency};
use crate::engine::fix_command::{FixCommand, fix_command};
use crate::engine::run_command;
use crate::model::Advisory;
use crate::theme::{Theme, fonts, metrics};

const MODAL_WIDTH: gpui::Pixels = px(620.);
/// Avis listés ; les suivants sont seulement comptés.
const SHOWN_ADVISORIES: usize = 4;
const OUTPUT_MAX_H: gpui::Pixels = px(200.);

/// Exécution de la commande.
enum RunState {
    Running,
    /// Terminée : réussie ou non.
    Done(bool),
    /// Le shell n'a pas pu être lancé.
    Failed(SharedString),
}

/// Modale ouverte sur un paquet.
pub(super) struct DependencyModal {
    dep: Dependency,
    advisories: Vec<Advisory>,
    /// Version qui corrige tous ses avis.
    fix: Option<String>,
    command: Option<FixCommand>,
    /// Dossier du projet, où la commande s'exécute.
    root: PathBuf,
    run: Option<RunState>,
    /// Début de l'exécution (temps écoulé sous la barre de chargement).
    started: Option<Instant>,
    output: Arc<Mutex<String>>,
    copied: bool,
    focus: FocusHandle,
    _ticker: Option<Task<()>>,
}

impl HeraldApp {
    /// Ouvre la modale d'un paquet du projet affiché.
    pub(super) fn open_dependency_modal(
        &mut self,
        dep: Dependency,
        advisories: Vec<Advisory>,
        fix: Option<String>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.project() else {
            return;
        };
        let root = project.path.clone();
        let all = self
            .dependencies
            .get(&root)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let command = fix_command(&dep, fix.as_deref(), all);
        let focus = cx.focus_handle();
        window.focus(&focus);
        self.dependency_modal = Some(DependencyModal {
            dep,
            advisories,
            fix,
            command,
            root,
            run: None,
            started: None,
            output: Arc::new(Mutex::new(String::new())),
            copied: false,
            focus,
            _ticker: None,
        });
        cx.notify();
    }

    fn close_dependency_modal(&mut self, cx: &mut Context<Self>) {
        // Pendant l'exécution, on garde la modale : la sortie reste visible.
        if matches!(
            self.dependency_modal.as_ref().and_then(|m| m.run.as_ref()),
            Some(RunState::Running)
        ) {
            return;
        }
        self.dependency_modal = None;
        cx.notify();
    }

    fn copy_modal_command(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = &mut self.dependency_modal else {
            return;
        };
        if let Some(command) = &modal.command {
            cx.write_to_clipboard(ClipboardItem::new_string(command.command.clone()));
            modal.copied = true;
            cx.notify();
        }
    }

    /// Exécute la commande dans le dossier du projet, en affichant la sortie
    /// au fil de l'eau.
    fn run_modal_command(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = &mut self.dependency_modal else {
            return;
        };
        let Some(command) = modal.command.as_ref().map(|c| c.command.clone()) else {
            return;
        };
        if matches!(modal.run, Some(RunState::Running)) {
            return;
        }
        let output = Arc::new(Mutex::new(String::new()));
        modal.output = Arc::clone(&output);
        modal.run = Some(RunState::Running);
        modal.started = Some(Instant::now());
        let root = modal.root.clone();

        // Rafraîchit la sortie tant que la commande tourne.
        modal._ticker = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let running = this
                    .update(cx, |app, cx| {
                        cx.notify();
                        matches!(
                            app.dependency_modal.as_ref().and_then(|m| m.run.as_ref()),
                            Some(RunState::Running)
                        )
                    })
                    .unwrap_or(false);
                if !running {
                    break;
                }
            }
        }));
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { run_command::run(&root, &command, output) })
                .await;
            this.update(cx, |app, cx| {
                if let Some(modal) = &mut app.dependency_modal {
                    modal.run = Some(match result {
                        Ok(success) => RunState::Done(success),
                        Err(message) => RunState::Failed(message.into()),
                    });
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Après une correction : relance l'analyse pour relire les versions.
    fn rerun_after_fix(&mut self, cx: &mut Context<Self>) {
        self.dependency_modal = None;
        self.start_analysis(cx);
    }

    pub(super) fn render_dependency_modal(
        &self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let modal = self.dependency_modal.as_ref()?;
        let dep = &modal.dep;
        let running = matches!(modal.run, Some(RunState::Running));
        let vulnerable = !modal.advisories.is_empty();

        let version = if dep.resolved {
            dep.version.clone()
        } else {
            format!("≥ {}", dep.version)
        };
        let status = if vulnerable {
            format!(
                "{} · {}",
                count_label(
                    modal.advisories.len() as u32,
                    "vulnérabilité connue",
                    "vulnérabilités connues"
                ),
                match &modal.fix {
                    Some(fix) => format!("corrigées en {fix}"),
                    None => "pas encore de version corrigée".to_owned(),
                }
            )
        } else {
            "Aucune vulnérabilité connue. La commande met le paquet à jour.".to_owned()
        };

        let header = div()
            .flex()
            .items_start()
            .gap(metrics::SPACE_3)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(metrics::TEXT_LG)
                            .font_weight(FontWeight::SEMIBOLD)
                            .font_family(fonts::MONO)
                            .child(format!("{} {version}", dep.name)),
                    )
                    .child(
                        div()
                            .text_size(metrics::TEXT_SM)
                            .text_color(theme.ink_muted)
                            .child(format!(
                                "{} · {} · {}",
                                dep.ecosystem,
                                if dep.direct { "directe" } else { "transitive" },
                                dep.source
                            )),
                    ),
            )
            .child(
                div()
                    .id("dependency-modal-close")
                    .p(metrics::SPACE_1)
                    .rounded(metrics::RADIUS)
                    .cursor_pointer()
                    .hover(move |style| style.bg(theme.bg_hover))
                    .on_click(cx.listener(|app, _, _, cx| app.close_dependency_modal(cx)))
                    .child(Icon::new(IconName::Close, theme.ink_soft).size(metrics::ICON_SM)),
            );

        let advisories = div()
            .flex()
            .flex_col()
            .gap(metrics::SPACE_2)
            .children(
                modal
                    .advisories
                    .iter()
                    .take(SHOWN_ADVISORIES)
                    .enumerate()
                    .map(|(ix, advisory)| advisory_item(ix, advisory, theme)),
            )
            .when(modal.advisories.len() > SHOWN_ADVISORIES, |this| {
                this.child(
                    div()
                        .text_size(metrics::TEXT_SM)
                        .text_color(theme.ink_muted)
                        .child(format!(
                            "et {}",
                            count_label(
                                (modal.advisories.len() - SHOWN_ADVISORIES) as u32,
                                "autre avis",
                                "autres avis"
                            )
                        )),
                )
            });

        let command_block = match &modal.command {
            Some(command) => div()
                .flex()
                .flex_col()
                .gap(metrics::SPACE_2)
                .child(caption("Commande", theme))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(metrics::SPACE_2)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .px(metrics::SPACE_3)
                                .py(metrics::SPACE_2)
                                .rounded(metrics::RADIUS)
                                .border_1()
                                .border_color(theme.border)
                                .bg(theme.code_bg)
                                .font_family(fonts::MONO)
                                .text_size(metrics::TEXT_SM)
                                .text_color(theme.code_text)
                                .child(command.command.clone()),
                        )
                        .child(
                            Button::new(
                                "dependency-modal-copy",
                                if modal.copied { "Copiée" } else { "Copier" },
                                theme,
                            )
                            .variant(ButtonVariant::Ghost)
                            .on_click(cx.listener(|app, _, _, cx| app.copy_modal_command(cx))),
                        ),
                )
                .children(command.note.clone().map(|note| {
                    div()
                        .text_size(metrics::TEXT_SM)
                        .text_color(theme.ink_muted)
                        .child(note)
                }))
                .child(
                    div()
                        .text_size(metrics::TEXT_SM)
                        .text_color(theme.ink_soft)
                        .child(format!(
                            "« Corriger » l'exécute dans {}, avec votre shell.",
                            modal.root.display()
                        )),
                ),
            None => div()
                .text_size(metrics::TEXT_SM)
                .text_color(theme.ink_muted)
                .child(format!(
                    "Herald ne connaît pas encore l'outil de l'écosystème {}.",
                    dep.ecosystem
                )),
        };

        let progress = running.then(|| {
            let elapsed = modal.started.map_or(0, |start| start.elapsed().as_secs());
            loading_bar(elapsed, theme)
        });

        let output = modal.run.as_ref().map(|_| {
            let text = modal.output.lock().map(|t| t.clone()).unwrap_or_default();
            div()
                .id("dependency-modal-output")
                .max_h(OUTPUT_MAX_H)
                .overflow_y_scroll()
                .p(metrics::SPACE_3)
                .rounded(metrics::RADIUS)
                .bg(theme.code_bg)
                .border_1()
                .border_color(theme.border_soft)
                .font_family(fonts::MONO)
                .text_size(metrics::TEXT_XS)
                .text_color(theme.code_text)
                .children(
                    text.lines()
                        .map(|line| div().child(line.to_owned()))
                        .collect::<Vec<_>>(),
                )
        });

        let result = match &modal.run {
            Some(RunState::Done(true)) => Some((
                theme.success,
                "Correction appliquée. Relancez l'analyse pour relire les versions installées."
                    .to_owned(),
            )),
            Some(RunState::Done(false)) => Some((
                theme.danger,
                "La commande a échoué. Lisez la sortie ci-dessus, puis réessayez ou corrigez à la \
                 main."
                    .to_owned(),
            )),
            Some(RunState::Failed(message)) => Some((theme.danger, message.to_string())),
            _ => None,
        };

        let has_command = modal.command.is_some();
        let succeeded = matches!(modal.run, Some(RunState::Done(true)));
        let action_label = match (&modal.run, vulnerable) {
            (Some(RunState::Running), _) => "Correction en cours…",
            (Some(RunState::Done(false) | RunState::Failed(_)), _) => "Réessayer",
            (_, true) => "Corriger",
            (_, false) => "Mettre à jour",
        };
        let footer = div()
            .flex()
            .items_center()
            .justify_end()
            .gap(metrics::SPACE_2)
            .child(
                Button::new("dependency-modal-cancel", "Fermer", theme)
                    .variant(ButtonVariant::Ghost)
                    .on_click(cx.listener(|app, _, _, cx| app.close_dependency_modal(cx))),
            )
            .child(if succeeded {
                Button::new("dependency-modal-rerun", "Relancer l'analyse", theme)
                    .variant(ButtonVariant::Primary)
                    .icon(IconName::Refresh)
                    .on_click(cx.listener(|app, _, _, cx| app.rerun_after_fix(cx)))
                    .into_any_element()
            } else if has_command {
                div()
                    .when(running, |this| this.opacity(0.6))
                    .child(
                        Button::new("dependency-modal-run", action_label, theme)
                            .variant(ButtonVariant::Primary)
                            .icon(IconName::Play)
                            .on_click(cx.listener(|app, _, _, cx| app.run_modal_command(cx))),
                    )
                    .into_any_element()
            } else {
                div().into_any_element()
            });

        Some(
            div()
                .id("dependency-modal-overlay")
                .track_focus(&modal.focus)
                .on_key_down(cx.listener(|app, event: &KeyDownEvent, _, cx| {
                    if event.keystroke.key == "escape" {
                        app.close_dependency_modal(cx);
                    }
                }))
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.ink.opacity(0.35))
                .on_click(cx.listener(|app, _, _, cx| app.close_dependency_modal(cx)))
                .child(
                    div()
                        .id("dependency-modal")
                        .w(MODAL_WIDTH)
                        .max_h(gpui::relative(0.9))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap(metrics::SPACE_4)
                        .p(metrics::SPACE_6)
                        .rounded(metrics::RADIUS_LG)
                        .bg(theme.bg_surface)
                        .border_1()
                        .border_color(theme.border)
                        .shadow_lg()
                        // Un clic dans la modale ne la ferme pas.
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(header)
                        .child(
                            div()
                                .text_size(metrics::TEXT_SM)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(if vulnerable {
                                    theme.danger
                                } else {
                                    theme.success
                                })
                                .child(status),
                        )
                        .when(vulnerable, |this| this.child(advisories))
                        .when(!dep.resolved, |this| {
                            this.child(
                                div()
                                    .text_size(metrics::TEXT_SM)
                                    .text_color(theme.ink_muted)
                                    .child(format!(
                                        "{} déclare {} {} au minimum, sans lockfile. La version \
                                         installée peut déjà être plus récente.",
                                        dep.source, dep.name, dep.version
                                    )),
                            )
                        })
                        .child(command_block)
                        .children(progress)
                        .children(output)
                        .children(result.map(|(color, text)| {
                            div()
                                .text_size(metrics::TEXT_SM)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(color)
                                .child(text)
                        }))
                        .child(footer),
                )
                .into_any_element(),
        )
    }
}

/// Barre de chargement indéterminée (un segment doré qui va et vient) et
/// temps écoulé : la commande ne dit pas où elle en est.
fn loading_bar(elapsed_secs: u64, theme: Theme) -> impl IntoElement {
    let elapsed = if elapsed_secs < 60 {
        format!("{elapsed_secs} s")
    } else {
        format!("{} min {:02} s", elapsed_secs / 60, elapsed_secs % 60)
    };
    div()
        .flex()
        .flex_col()
        .gap(metrics::SPACE_2)
        .child(
            div()
                .flex()
                .justify_between()
                .text_size(metrics::TEXT_SM)
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child("Correction en cours…"),
                )
                .child(div().text_color(theme.ink_muted).child(elapsed)),
        )
        .child(
            div()
                .relative()
                .w_full()
                .h(px(6.))
                .rounded_full()
                .overflow_hidden()
                .bg(theme.track)
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .h_full()
                        .w(gpui::relative(SEGMENT))
                        .rounded_full()
                        .bg(theme.halo)
                        .with_animation(
                            "dependency-fix-progress",
                            Animation::new(Duration::from_millis(1400)).repeat(),
                            |segment, delta| {
                                // Aller puis retour, du bord gauche au bord droit.
                                let t = if delta < 0.5 {
                                    delta * 2.
                                } else {
                                    (1. - delta) * 2.
                                };
                                segment.left(gpui::relative(t * (1. - SEGMENT)))
                            },
                        ),
                ),
        )
}

/// Largeur du segment de la barre de chargement (fraction de la piste).
const SEGMENT: f32 = 0.3;

fn caption(text: &str, theme: Theme) -> impl IntoElement {
    div()
        .text_size(metrics::TEXT_XS)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.ink_soft)
        .child(text.to_uppercase())
}

/// Un avis OSV dans la modale : gravité, identifiant, résumé, lien.
fn advisory_item(ix: usize, advisory: &Advisory, theme: Theme) -> AnyElement {
    let url = advisory.url.clone();
    let cvss = advisory
        .cvss
        .map(|score| format!("CVSS {score:.1}").replace('.', ","));
    let published = cve::published_label(advisory.published_unix, cve::unix_now());
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .p(metrics::SPACE_3)
        .rounded(metrics::RADIUS)
        .bg(theme.bg_page)
        .child(
            div()
                .flex()
                .items_center()
                .gap(metrics::SPACE_3)
                .text_size(metrics::TEXT_SM)
                .text_color(theme.ink_muted)
                .child(SeverityLabel::new(advisory.severity, theme))
                .child(div().font_family(fonts::MONO).child(advisory.id.clone()))
                .children(cvss)
                .child(div().flex_1())
                .child(format!("publiée {published}"))
                .child(
                    Button::new(("dependency-modal-advisory", ix), "Voir l'avis", theme)
                        .variant(ButtonVariant::Ghost)
                        .on_click(move |_, _, cx| cx.open_url(&url)),
                ),
        )
        .child(
            div()
                .text_size(metrics::TEXT_SM)
                .child(advisory.summary.clone()),
        )
        .into_any_element()
}

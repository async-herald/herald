//! Modale « Nouveau projet » : choix du type de projet à ajouter.
//!
//! - **Dossier local :** sélecteur de dossier natif.
//! - **Dépôt Git :** clonage d'une URL HTTPS dans le dossier de l'app, puis ajout
//!   du dossier cloné (mis à jour avant chaque analyse). L'analyse reste locale.
//! - **Projet de démo :** le dépôt d'exemple `async-herald/demo-repo`, cloné de même.

use gpui::{
    ClipboardItem, Entity, FocusHandle, Focusable, FontWeight, KeyDownEvent, SharedString,
    Subscription, Window, div, prelude::*, px,
};

use super::app::HeraldApp;
use super::components::{
    Button, ButtonVariant, Icon, IconName, ProgressBar, TextInput, TextInputEvent,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::engine::workspace::{self, CloneError, CloneProgress, DEMO_REPOSITORY};
use crate::model::Project;
use crate::theme::{Theme, metrics};

const MODAL_WIDTH: gpui::Pixels = px(460.);
const OPTION_ICON_BOX: gpui::Pixels = px(36.);

/// État de la modale ouverte.
pub(super) struct NewProjectModal {
    step: Step,
    focus: FocusHandle,
    url_input: Entity<TextInput>,
    busy: bool,
    error: Option<SharedString>,
    /// Échec du dernier clonage, avec l'aide pour un dépôt privé.
    clone_error: Option<CloneError>,
    /// Avancement du clonage en cours, écrit par le fil de Git.
    progress: Arc<Mutex<CloneProgress>>,
    /// Redessine la barre pendant le clonage.
    _ticker: Option<gpui::Task<()>>,
    _subscription: Subscription,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Choose,
    GitUrl,
}

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Local,
    Git,
    Demo,
}

impl Kind {
    pub(super) const ALL: [Self; 3] = [Self::Local, Self::Git, Self::Demo];

    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Local => "new-project-local",
            Self::Git => "new-project-git",
            Self::Demo => "new-project-demo",
        }
    }

    pub(super) fn icon(self) -> IconName {
        match self {
            Self::Local => IconName::FolderUp,
            Self::Git => IconName::GitBranch,
            Self::Demo => IconName::Sparkles,
        }
    }

    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Local => "Dossier local",
            Self::Git => "Dépôt Git",
            Self::Demo => "Projet de démo",
        }
    }

    pub(super) fn description(self) -> &'static str {
        match self {
            Self::Local => "Un dossier de code déjà présent sur cette machine.",
            Self::Git => "Cloner un dépôt GitHub, GitLab ou Bitbucket, puis l'analyser ici.",
            Self::Demo => "Le dépôt d'exemple async-herald/demo-repo, pour découvrir Herald.",
        }
    }
}

impl HeraldApp {
    /// Ouvre la modale de choix du type de projet.
    pub(super) fn open_new_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url_input = cx.new(|cx| TextInput::new("https://github.com/organisation/depot", cx));
        let subscription =
            cx.subscribe_in(&url_input, window, |app, _, event, _, cx| match event {
                TextInputEvent::Submit => app.clone_repository(cx),
            });
        let modal = NewProjectModal {
            step: Step::Choose,
            focus: cx.focus_handle(),
            url_input,
            busy: false,
            error: None,
            clone_error: None,
            progress: Arc::default(),
            _ticker: None,
            _subscription: subscription,
        };
        window.focus(&modal.focus);
        self.new_project = Some(modal);
        cx.notify();
    }

    /// Démarre directement un type de projet (cartes de l'accueil).
    pub(super) fn start_project_kind(
        &mut self,
        kind: Kind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match kind {
            Kind::Local => self.pick_folder(cx),
            Kind::Git | Kind::Demo => {
                self.open_new_project(window, cx);
                self.choose_kind(kind, window, cx);
            }
        }
    }

    fn close_new_project(&mut self, cx: &mut Context<Self>) {
        // Un clonage en cours se termine en arrière-plan ; son résultat est alors ignoré.
        self.new_project = None;
        cx.notify();
    }

    fn choose_kind(&mut self, kind: Kind, window: &mut Window, cx: &mut Context<Self>) {
        match kind {
            Kind::Local => {
                self.close_new_project(cx);
                self.pick_folder(cx);
            }
            Kind::Git => {
                let Some(modal) = &mut self.new_project else {
                    return;
                };
                modal.step = Step::GitUrl;
                modal.error = None;
                let handle = modal.url_input.focus_handle(cx);
                window.focus(&handle);
                cx.notify();
            }
            Kind::Demo => {
                let Some(modal) = &mut self.new_project else {
                    return;
                };
                modal.step = Step::GitUrl;
                modal.url_input.update(cx, |input, cx| {
                    input.set_text(DEMO_REPOSITORY, cx);
                });
                self.clone_repository(cx);
            }
        }
    }

    fn back_to_choices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(modal) = &mut self.new_project {
            if modal.busy {
                return;
            }
            modal.step = Step::Choose;
            modal.error = None;
            window.focus(&modal.focus);
            cx.notify();
        }
    }

    /// Clone le dépôt saisi en arrière-plan, puis l'ajoute comme projet local.
    fn clone_repository(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = &mut self.new_project else {
            return;
        };
        if modal.busy {
            return;
        }
        let url = modal.url_input.read(cx).text().trim().to_owned();
        let Some(name) = workspace::repository_name(&url) else {
            modal.error = Some(
                "Collez l'adresse HTTPS d'un dépôt, par exemple https://github.com/org/depot."
                    .into(),
            );
            cx.notify();
            return;
        };
        // Dépôt déjà ajouté : on le sélectionne plutôt que de le recloner.
        if let Some(ix) = self
            .projects
            .iter()
            .position(|project| project.remote.as_deref() == Some(url.as_str()))
        {
            self.new_project = None;
            self.select_project(ix, cx);
            self.screen = self.project_screen();
            cx.notify();
            return;
        }
        let Some(modal) = &mut self.new_project else {
            return;
        };
        let Some(dest) = workspace::clone_destination(&name) else {
            modal.error = Some("Dossier personnel introuvable.".into());
            cx.notify();
            return;
        };
        modal.busy = true;
        modal.error = None;
        modal.clone_error = None;
        let progress = Arc::new(Mutex::new(CloneProgress {
            step: "Connexion au dépôt",
            ..CloneProgress::default()
        }));
        modal.progress = Arc::clone(&progress);
        modal._ticker = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                let busy = this
                    .update(cx, |app, cx| {
                        cx.notify();
                        app.new_project.as_ref().is_some_and(|modal| modal.busy)
                    })
                    .unwrap_or(false);
                if !busy {
                    break;
                }
            }
        }));
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    workspace::git_clone(&url, &dest, |update| {
                        if let Ok(mut current) = progress.lock() {
                            // La barre ne recule jamais (Git enchaîne plusieurs étapes).
                            if update.percent >= current.percent {
                                *current = update;
                            }
                        }
                    })
                    .map(|()| (dest, url))
                })
                .await;
            this.update(cx, |app, cx| {
                // Modale fermée entre-temps : l'utilisateur a abandonné.
                let Some(modal) = &mut app.new_project else {
                    return;
                };
                match result {
                    Ok((dest, url)) => {
                        app.new_project = None;
                        app.add_existing_project(Project::cloned(dest, url), cx);
                    }
                    Err(error) => {
                        modal.busy = false;
                        modal.error = None;
                        modal.clone_error = Some(error);
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    /// Voile et modale, par-dessus l'interface.
    pub(super) fn render_new_project(
        &self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let modal = self.new_project.as_ref()?;
        let body = match modal.step {
            Step::Choose => choices(theme, cx).into_any_element(),
            Step::GitUrl => git_form(modal, theme, cx).into_any_element(),
        };
        let title = match modal.step {
            Step::Choose => "Nouveau projet",
            Step::GitUrl => "Cloner un dépôt Git",
        };

        Some(
            div()
                .id("new-project-overlay")
                .track_focus(&modal.focus)
                .on_key_down(cx.listener(|app, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        match app.new_project.as_ref().map(|modal| modal.step) {
                            Some(Step::GitUrl) => app.back_to_choices(window, cx),
                            _ => app.close_new_project(cx),
                        }
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
                .on_click(cx.listener(|app, _, _, cx| app.close_new_project(cx)))
                .child(
                    div()
                        .id("new-project-modal")
                        .w(MODAL_WIDTH)
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
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(metrics::TEXT_LG)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(title),
                                )
                                .child(
                                    div()
                                        .id("new-project-close")
                                        .p(metrics::SPACE_1)
                                        .rounded(metrics::RADIUS)
                                        .cursor_pointer()
                                        .hover(move |style| style.bg(theme.bg_hover))
                                        .on_click(
                                            cx.listener(|app, _, _, cx| app.close_new_project(cx)),
                                        )
                                        .child(
                                            Icon::new(IconName::Close, theme.ink_soft)
                                                .size(metrics::ICON_SM),
                                        ),
                                ),
                        )
                        .child(body),
                ),
        )
    }
}

fn choices(theme: Theme, cx: &mut Context<HeraldApp>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(metrics::SPACE_2)
        .child(
            div()
                .text_color(theme.ink_muted)
                .child("Quel type de projet voulez-vous analyser ?"),
        )
        .children(Kind::ALL.into_iter().map(|kind| {
            div()
                .id(kind.id())
                .flex()
                .items_center()
                .gap(metrics::SPACE_3)
                .p(metrics::SPACE_3)
                .rounded(metrics::RADIUS_LG)
                .border_1()
                .border_color(theme.border)
                .cursor_pointer()
                .hover(move |style| style.bg(theme.bg_hover).border_color(theme.accent))
                .on_click(cx.listener(move |app, _, window, cx| app.choose_kind(kind, window, cx)))
                .child(
                    div()
                        .flex_none()
                        .size(OPTION_ICON_BOX)
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(metrics::RADIUS)
                        .bg(theme.bg_active)
                        .child(Icon::new(kind.icon(), theme.accent_text)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(kind.title()))
                        .child(
                            div()
                                .text_size(metrics::TEXT_SM)
                                .text_color(theme.ink_muted)
                                .child(kind.description()),
                        ),
                )
        }))
        .child(
            div()
                .pt(metrics::SPACE_1)
                .text_size(metrics::TEXT_SM)
                .text_color(theme.ink_soft)
                .child(
                    "Astuce : vous pouvez aussi déposer un dossier n'importe où dans la fenêtre.",
                ),
        )
}

fn git_form(
    modal: &NewProjectModal,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> impl IntoElement {
    let submit = if modal.busy {
        "Clonage…"
    } else {
        "Cloner et ajouter"
    };
    let progress = modal.busy.then(|| {
        let current = modal.progress.lock().map(|p| p.clone()).unwrap_or_default();
        let mut label = format!("{} · {} %", current.step, current.percent);
        if !current.detail.is_empty() {
            label.push_str(&format!(" · {}", current.detail));
        }
        div()
            .flex()
            .flex_col()
            .gap(metrics::SPACE_1)
            .child(ProgressBar::new(f32::from(current.percent) / 100., theme))
            .child(
                div()
                    .text_size(metrics::TEXT_SM)
                    .text_color(theme.ink_muted)
                    .child(label),
            )
    });
    div()
        .flex()
        .flex_col()
        .gap(metrics::SPACE_3)
        .child(div().text_color(theme.ink_muted).child(
            "Le dépôt est copié sur cette machine, puis analysé en local. \
             Seuls les dépôts publics sont pris en charge pour l'instant.",
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(metrics::SPACE_2)
                .child(
                    div()
                        .text_size(metrics::TEXT_SM)
                        .font_weight(FontWeight::MEDIUM)
                        .child("Adresse du dépôt"),
                )
                .child(modal.url_input.clone())
                .children(progress)
                .children(modal.error.clone().map(|error| {
                    div()
                        .text_size(metrics::TEXT_SM)
                        .text_color(theme.danger)
                        .child(error)
                }))
                .children(
                    modal
                        .clone_error
                        .as_ref()
                        .map(|error| clone_error_help(error, theme)),
                ),
        )
        .child(
            div()
                .flex()
                .justify_end()
                .gap(metrics::SPACE_2)
                .child(
                    Button::new("new-project-back", "Retour", theme)
                        .variant(ButtonVariant::Ghost)
                        .on_click(
                            cx.listener(|app, _, window, cx| app.back_to_choices(window, cx)),
                        ),
                )
                .child(
                    Button::new("new-project-clone", submit, theme)
                        .variant(ButtonVariant::Primary)
                        .on_click(cx.listener(|app, _, _, cx| app.clone_repository(cx))),
                ),
        )
}

/// Explication d'un clonage raté et, pour un dépôt privé, les étapes à suivre.
fn clone_error_help(error: &CloneError, theme: Theme) -> impl IntoElement + use<> {
    let steps = error.help_steps();
    let command = error.command.clone();
    div()
        .flex()
        .flex_col()
        .gap(metrics::SPACE_2)
        .text_size(metrics::TEXT_SM)
        .child(div().text_color(theme.danger).child(error.message.clone()))
        .children(error.detail.clone().map(|detail| {
            div()
                .font_family(crate::theme::fonts::MONO)
                .text_color(theme.ink_muted)
                .child(format!("Git : {detail}"))
        }))
        .when(!steps.is_empty(), |this| {
            this.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(metrics::SPACE_1)
                    .p(metrics::SPACE_3)
                    .rounded(metrics::RADIUS)
                    .bg(theme.bg_hover)
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Pour cloner un dépôt privé"),
                    )
                    .children(steps.into_iter().enumerate().map(|(ix, step)| {
                        div()
                            .flex()
                            .gap(metrics::SPACE_2)
                            .text_color(theme.ink_muted)
                            .child(div().flex_none().child(format!("{}.", ix + 1)))
                            .child(div().flex_1().min_w_0().child(step))
                    }))
                    .children(command.map(|command| {
                        let copied = command.clone();
                        div()
                            .flex()
                            .items_center()
                            .gap(metrics::SPACE_2)
                            .mt(metrics::SPACE_1)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .px(metrics::SPACE_2)
                                    .py(metrics::SPACE_1)
                                    .rounded(metrics::RADIUS)
                                    .bg(theme.bg_surface)
                                    .border_1()
                                    .border_color(theme.border_soft)
                                    .font_family(crate::theme::fonts::MONO)
                                    .text_color(theme.ink)
                                    .child(command),
                            )
                            .child(
                                Button::new("copy-clone-command", "Copier", theme)
                                    .variant(ButtonVariant::Default)
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            copied.clone(),
                                        ));
                                    }),
                            )
                    }))
                    .child(
                        div()
                            .mt(metrics::SPACE_1)
                            .text_color(theme.ink_muted)
                            .child(
                                "Ou clonez-le vous-même puis ajoutez-le avec « Dossier local ».",
                            ),
                    ),
            )
        })
}

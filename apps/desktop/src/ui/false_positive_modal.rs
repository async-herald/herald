//! Modale « Faux positif » (plan de correction) : la règle signalée, un
//! extrait du code à l'endroit relevé, joint au signalement seulement si
//! l'on coche la case de consentement, et un commentaire libre. « Envoyer le
//! signalement » le transmet à herald.codes (page admin > Faux positifs).

use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, Entity, FocusHandle, Focusable, FontWeight, KeyDownEvent, SharedString, div,
    prelude::*, px,
};

use super::app::HeraldApp;
use super::components::{Button, ButtonVariant, CodeArea, Icon, IconName, SeverityLabel};
use crate::model::Severity;
use crate::theme::{Theme, fonts, metrics};

const MODAL_WIDTH: gpui::Pixels = px(640.);
/// Lignes montrées de part et d'autre de la ligne relevée.
const CONTEXT_LINES: u32 = 3;
const COMMENT_HEIGHT: gpui::Pixels = px(112.);
/// Au-delà, une ligne d'extrait est coupée (fichiers minifiés).
const MAX_LINE_CHARS: usize = 160;

/// Un endroit relevé par la règle.
struct Occurrence {
    file: SharedString,
    line: Option<u32>,
}

/// Lignes de code autour de l'endroit relevé.
struct Excerpt {
    /// (numéro de ligne, texte)
    lines: Vec<(u32, String)>,
    target: u32,
}

/// Envoi du signalement.
enum SendState {
    Idle,
    Sending,
    Sent,
    /// Message à afficher (hors ligne, trop d'envois…).
    Failed(String),
}

/// Ce qui part vers herald.codes. Ni le nom du projet ni son chemin sur la
/// machine : seulement le chemin du fichier dans le projet.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FalsePositivePayload {
    rule: String,
    rule_title: String,
    severity: &'static str,
    file: String,
    line: Option<u32>,
    comment: String,
    /// Seulement avec le consentement de l'utilisateur.
    excerpt: Option<String>,
    excerpt_start_line: Option<u32>,
    app_version: &'static str,
}

/// Modale ouverte sur une correction du plan.
pub(super) struct FalsePositiveModal {
    title: SharedString,
    rule: SharedString,
    severity: Severity,
    root: PathBuf,
    occurrences: Vec<Occurrence>,
    current: usize,
    excerpt: Option<Excerpt>,
    /// Consentement à envoyer l'extrait avec le signalement (non coché par défaut).
    consent: bool,
    comment: Entity<CodeArea>,
    focus: FocusHandle,
    state: SendState,
}

impl HeraldApp {
    /// Ouvre la modale de signalement d'une correction du plan.
    pub(super) fn open_false_positive(
        &mut self,
        gi: usize,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(project), Some(report)) = (self.project(), self.report()) else {
            return;
        };
        let Some(group) = report.group(gi) else {
            return;
        };
        let occurrences: Vec<Occurrence> = group
            .occurrences
            .iter()
            .filter_map(|&ix| report.issues.get(ix))
            .map(|issue| Occurrence {
                file: issue.file.clone(),
                line: issue.line,
            })
            .collect();
        let root = project.path.clone();
        let excerpt = occurrences.first().and_then(|o| read_excerpt(&root, o));
        let comment = cx.new(|cx| CodeArea::new("", "commentaire.txt", cx).plain());
        let focus = cx.focus_handle();
        // Le curseur est tout de suite dans le commentaire.
        window.focus(&comment.read(cx).focus_handle(cx));
        self.false_positive = Some(FalsePositiveModal {
            title: group.title.clone(),
            rule: group.rule.clone(),
            severity: group.severity,
            root,
            occurrences,
            current: 0,
            excerpt,
            consent: false,
            comment,
            focus,
            state: SendState::Idle,
        });
        cx.notify();
    }

    fn close_false_positive(&mut self, cx: &mut Context<Self>) {
        // Pendant l'envoi, on garde la modale : la réponse arrive ici.
        if matches!(
            self.false_positive.as_ref().map(|m| &m.state),
            Some(SendState::Sending)
        ) {
            return;
        }
        self.false_positive = None;
        cx.notify();
    }

    /// Envoie le signalement à herald.codes, hors du fil de l'interface.
    fn send_false_positive(&mut self, cx: &mut Context<Self>) {
        let service = self.report_service();
        let Some(modal) = &mut self.false_positive else {
            return;
        };
        if matches!(modal.state, SendState::Sending | SendState::Sent) {
            return;
        }
        let Some(occurrence) = modal.occurrences.get(modal.current) else {
            return;
        };
        let excerpt = modal.excerpt.as_ref().filter(|_| modal.consent);
        let payload = FalsePositivePayload {
            rule: modal.rule.to_string(),
            rule_title: modal.title.to_string(),
            severity: severity_code(modal.severity),
            file: occurrence.file.to_string(),
            line: occurrence.line,
            comment: modal.comment.read(cx).text().trim().to_owned(),
            excerpt: excerpt.map(|excerpt| {
                excerpt
                    .lines
                    .iter()
                    .map(|(_, text)| text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            }),
            excerpt_start_line: excerpt.and_then(|excerpt| excerpt.lines.first().map(|(n, _)| *n)),
            app_version: env!("CARGO_PKG_VERSION"),
        };
        modal.state = SendState::Sending;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { service.send(&payload) })
                .await;
            this.update(cx, |app, cx| {
                if let Some(modal) = &mut app.false_positive {
                    modal.state = match result {
                        Ok(()) => SendState::Sent,
                        Err(message) => SendState::Failed(message),
                    };
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn toggle_excerpt_consent(&mut self, cx: &mut Context<Self>) {
        if let Some(modal) = &mut self.false_positive {
            modal.consent = !modal.consent;
            cx.notify();
        }
    }

    /// Passe à l'occurrence précédente (-1) ou suivante (+1) de la règle.
    fn step_occurrence(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(modal) = &mut self.false_positive else {
            return;
        };
        let Some(next) = modal.current.checked_add_signed(delta) else {
            return;
        };
        if next >= modal.occurrences.len() {
            return;
        }
        modal.current = next;
        modal.excerpt = read_excerpt(&modal.root, &modal.occurrences[next]);
        cx.notify();
    }

    pub(super) fn render_false_positive(
        &self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let modal = self.false_positive.as_ref()?;
        let occurrence = &modal.occurrences.get(modal.current)?;
        let count = modal.occurrences.len();

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
                            .flex()
                            .items_center()
                            .gap(metrics::SPACE_2)
                            .child(
                                Icon::new(IconName::Flag, theme.ink_muted).size(metrics::ICON_SM),
                            )
                            .child(
                                div()
                                    .text_size(metrics::TEXT_LG)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Signaler un faux positif"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(metrics::SPACE_2)
                            .text_size(metrics::TEXT_SM)
                            .child(SeverityLabel::new(modal.severity, theme))
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(modal.title.clone()),
                            )
                            .child(
                                div()
                                    .font_family(fonts::MONO)
                                    .text_size(metrics::TEXT_XS)
                                    .text_color(theme.ink_soft)
                                    .child(modal.rule.clone()),
                            ),
                    ),
            )
            .child(
                div()
                    .id("false-positive-close")
                    .p(metrics::SPACE_1)
                    .rounded(metrics::RADIUS)
                    .cursor_pointer()
                    .hover(move |style| style.bg(theme.bg_hover))
                    .on_click(cx.listener(|app, _, _, cx| app.close_false_positive(cx)))
                    .child(Icon::new(IconName::Close, theme.ink_soft).size(metrics::ICON_SM)),
            );

        let location: SharedString = match occurrence.line {
            Some(line) => format!("{}:{line}", occurrence.file).into(),
            None => occurrence.file.clone(),
        };

        // Navigation entre les endroits relevés par la règle.
        let stepper = (count > 1).then(|| {
            let arrow = |id: &'static str, icon: IconName, delta: isize, enabled: bool| {
                div()
                    .id(id)
                    .p(px(2.))
                    .rounded(metrics::RADIUS)
                    .when(enabled, |this| {
                        this.cursor_pointer()
                            .hover(move |style| style.bg(theme.bg_hover))
                            .on_click(
                                cx.listener(move |app, _, _, cx| app.step_occurrence(delta, cx)),
                            )
                    })
                    .when(!enabled, |this| this.opacity(0.35))
                    .child(Icon::new(icon, theme.ink_muted).size(metrics::ICON_SM))
            };
            div()
                .flex()
                .items_center()
                .gap(metrics::SPACE_1)
                .text_size(metrics::TEXT_SM)
                .text_color(theme.ink_muted)
                .child(arrow(
                    "false-positive-prev",
                    IconName::ChevronLeft,
                    -1,
                    modal.current > 0,
                ))
                .child(format!("{} sur {count}", modal.current + 1))
                .child(arrow(
                    "false-positive-next",
                    IconName::ChevronRight,
                    1,
                    modal.current + 1 < count,
                ))
        });

        // Consentement explicite avant de joindre le code au signalement.
        let consent = modal.excerpt.is_some().then(|| {
            let checked = modal.consent;
            div()
                .id("false-positive-consent")
                .flex()
                .items_start()
                .gap(metrics::SPACE_3)
                .p(metrics::SPACE_3)
                .rounded(metrics::RADIUS)
                .border_1()
                .border_color(theme.accent)
                .bg(theme.accent_soft)
                .cursor_pointer()
                .on_click(cx.listener(|app, _, _, cx| app.toggle_excerpt_consent(cx)))
                .child(
                    div()
                        .mt(px(1.))
                        .flex_none()
                        .size(px(18.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.))
                        .border_1()
                        .map(|this| {
                            if checked {
                                this.bg(theme.accent).border_color(theme.accent).child(
                                    Icon::new(IconName::Check, theme.on_accent).size(px(12.)),
                                )
                            } else {
                                this.bg(theme.bg_surface).border_color(theme.accent_text)
                            }
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(metrics::TEXT_BASE)
                                .font_weight(FontWeight::MEDIUM)
                                .child(
                                    "J'accepte d'envoyer cet extrait de code à l'équipe Herald.",
                                ),
                        )
                        .child(
                            div()
                                .text_size(metrics::TEXT_SM)
                                .text_color(theme.ink_muted)
                                .child("Il sert uniquement à corriger la règle,"),
                        )
                        .child(
                            div()
                                .text_size(metrics::TEXT_SM)
                                .text_color(theme.ink_muted)
                                .child("puis il est définitivement supprimé."),
                        ),
                )
        });

        let code: AnyElement = match &modal.excerpt {
            Some(excerpt) => div()
                .id("false-positive-excerpt")
                .overflow_x_scroll()
                .py(metrics::SPACE_2)
                .rounded(metrics::RADIUS)
                .bg(theme.code_bg)
                .border_1()
                .border_color(theme.border_soft)
                .font_family(fonts::MONO)
                .text_size(metrics::TEXT_SM)
                .line_height(metrics::SPACE_5)
                .children(excerpt.lines.iter().map(|(number, text)| {
                    let target = *number == excerpt.target;
                    div()
                        .flex()
                        .whitespace_nowrap()
                        .when(target, |this| this.bg(theme.code_line_warning))
                        .child(
                            div()
                                .w(px(44.))
                                .flex_none()
                                .pr(metrics::SPACE_3)
                                .flex()
                                .justify_end()
                                .text_color(if target {
                                    theme.warning
                                } else {
                                    theme.code_line_number
                                })
                                .child(number.to_string()),
                        )
                        .child(
                            div()
                                .pr(metrics::SPACE_3)
                                .text_color(theme.code_text)
                                .child(text.clone()),
                        )
                }))
                .into_any_element(),
            None => div()
                .text_size(metrics::TEXT_SM)
                .text_color(theme.ink_muted)
                .child(if occurrence.line.is_none() {
                    "Pas d'extrait : ce problème porte sur tout le fichier."
                } else {
                    "Le fichier n'a pas pu être lu."
                })
                .into_any_element(),
        };

        let excerpt_block = div()
            .flex()
            .flex_col()
            .gap(metrics::SPACE_2)
            .child(
                div()
                    .flex()
                    .items_center()
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
                                    .flex()
                                    .items_center()
                                    .gap(metrics::SPACE_2)
                                    .child(caption("Extrait de code", theme))
                                    // Badge : l'extrait part avec le signalement.
                                    .when(modal.consent && modal.excerpt.is_some(), |this| {
                                        this.child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(3.))
                                                .px(metrics::SPACE_2)
                                                .py(px(1.))
                                                .rounded_full()
                                                .bg(theme.accent)
                                                .text_size(metrics::TEXT_XS)
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(theme.on_accent)
                                                .child(
                                                    Icon::new(IconName::Check, theme.on_accent)
                                                        .size(px(10.)),
                                                )
                                                .child("Inclus"),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .font_family(fonts::MONO)
                                    .text_size(metrics::TEXT_SM)
                                    .text_color(theme.ink_muted)
                                    .child(location),
                            ),
                    )
                    .children(stepper),
            )
            .child(code)
            .children(consent);

        let comment_block = div()
            .flex()
            .flex_col()
            .gap(metrics::SPACE_2)
            .child(caption("Commentaire", theme))
            .child(
                div()
                    .text_size(metrics::TEXT_SM)
                    .text_color(theme.ink_muted)
                    .child("Pourquoi ce n'est pas un problème ici ?"),
            )
            .child(
                div()
                    .h(COMMENT_HEIGHT)
                    .p(metrics::SPACE_2)
                    .rounded(metrics::RADIUS)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.bg_surface)
                    .overflow_hidden()
                    .child(modal.comment.clone()),
            );

        let sending = matches!(modal.state, SendState::Sending);
        let error = match &modal.state {
            SendState::Failed(message) => Some(
                div()
                    .text_size(metrics::TEXT_SM)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.danger)
                    .child(message.clone()),
            ),
            _ => None,
        };
        let footer = div()
            .flex()
            .items_center()
            .justify_end()
            .gap(metrics::SPACE_2)
            .child(
                Button::new("false-positive-cancel", "Annuler", theme)
                    .variant(ButtonVariant::Ghost)
                    .on_click(cx.listener(|app, _, _, cx| app.close_false_positive(cx))),
            )
            .child(
                div().when(sending, |this| this.opacity(0.6)).child(
                    Button::new(
                        "false-positive-send",
                        match modal.state {
                            SendState::Sending => "Envoi…",
                            SendState::Failed(_) => "Réessayer",
                            _ => "Envoyer le signalement",
                        },
                        theme,
                    )
                    .variant(ButtonVariant::Primary)
                    .icon(IconName::Flag)
                    .on_click(cx.listener(|app, _, _, cx| app.send_false_positive(cx))),
                ),
            );

        // Envoyé : remerciement à la place du formulaire.
        let body: Vec<AnyElement> = if matches!(modal.state, SendState::Sent) {
            vec![
                header.into_any_element(),
                sent_notice(modal.consent && modal.excerpt.is_some(), theme, cx).into_any_element(),
            ]
        } else {
            vec![
                header.into_any_element(),
                excerpt_block.into_any_element(),
                comment_block.into_any_element(),
                div()
                    .flex()
                    .flex_col()
                    .gap(metrics::SPACE_2)
                    .children(error)
                    .child(footer)
                    .into_any_element(),
            ]
        };

        Some(
            div()
                .id("false-positive-overlay")
                .track_focus(&modal.focus)
                .on_key_down(cx.listener(|app, event: &KeyDownEvent, _, cx| {
                    if event.keystroke.key == "escape" {
                        app.close_false_positive(cx);
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
                .on_click(cx.listener(|app, _, _, cx| app.close_false_positive(cx)))
                .child(
                    div()
                        .id("false-positive-modal")
                        .w(MODAL_WIDTH)
                        .max_h(gpui::relative(0.9))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap(metrics::SPACE_5)
                        .p(metrics::SPACE_6)
                        .rounded(metrics::RADIUS_LG)
                        .bg(theme.bg_surface)
                        .border_1()
                        .border_color(theme.border)
                        .shadow_lg()
                        // Un clic dans la modale ne la ferme pas.
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .children(body),
                )
                .into_any_element(),
        )
    }
}

/// Gravité telle que l'API la reçoit.
fn severity_code(severity: Severity) -> &'static str {
    match severity {
        Severity::Critical => "critical",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
}

/// Signalement parti : remerciement et « Fermer ».
fn sent_notice(with_excerpt: bool, theme: Theme, cx: &mut Context<HeraldApp>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(metrics::SPACE_4)
        .child(
            div()
                .flex()
                .items_start()
                .gap(metrics::SPACE_3)
                .p(metrics::SPACE_4)
                .rounded(metrics::RADIUS)
                .bg(theme.success_bg)
                .child(Icon::new(IconName::Check, theme.success).size(metrics::ICON_SM))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.success)
                                .child("Merci, votre signalement est envoyé."),
                        )
                        .child(
                            div()
                                .text_size(metrics::TEXT_SM)
                                .text_color(theme.ink_muted)
                                .child("Nous le relisons pour corriger la règle."),
                        )
                        .when(with_excerpt, |this| {
                            this.child(
                                div()
                                    .text_size(metrics::TEXT_SM)
                                    .text_color(theme.ink_muted)
                                    .child("L'extrait de code sera supprimé une fois traité."),
                            )
                        }),
                ),
        )
        .child(
            div().flex().justify_end().child(
                Button::new("false-positive-done", "Fermer", theme)
                    .variant(ButtonVariant::Primary)
                    .on_click(cx.listener(|app, _, _, cx| app.close_false_positive(cx))),
            ),
        )
}

fn caption(text: &str, theme: Theme) -> impl IntoElement {
    div()
        .text_size(metrics::TEXT_XS)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.ink_soft)
        .child(text.to_uppercase())
}

/// Lignes autour de l'endroit relevé ; `None` sans ligne ou si le fichier
/// ne se lit pas (supprimé depuis l'analyse, binaire…).
fn read_excerpt(root: &Path, occurrence: &Occurrence) -> Option<Excerpt> {
    let target = occurrence.line?;
    let text = std::fs::read_to_string(root.join(occurrence.file.as_ref())).ok()?;
    excerpt_from(&text, target)
}

fn excerpt_from(text: &str, target: u32) -> Option<Excerpt> {
    let first = target.saturating_sub(CONTEXT_LINES).max(1);
    let last = target + CONTEXT_LINES;
    let lines: Vec<(u32, String)> = text
        .lines()
        .zip(1u32..)
        .filter(|(_, number)| (first..=last).contains(number))
        .map(|(line, number)| {
            let line = line.replace('\t', "    ");
            let line = match line.char_indices().nth(MAX_LINE_CHARS) {
                Some((cut, _)) => format!("{}…", &line[..cut]),
                None => line,
            };
            (number, line)
        })
        .collect();
    lines
        .iter()
        .any(|(number, _)| *number == target)
        .then_some(Excerpt { lines, target })
}

#[cfg(test)]
mod tests {
    use super::{FalsePositivePayload, excerpt_from};

    /// Le format attendu par le service pour un signalement de faux positif.
    #[test]
    fn the_payload_matches_the_api() {
        let payload = FalsePositivePayload {
            rule: "placeholder-assertion".into(),
            rule_title: "Vérification qui ne teste rien".into(),
            severity: "warning",
            file: "src/app.test.ts".into(),
            line: Some(12),
            comment: "Voulu".into(),
            excerpt: None,
            excerpt_start_line: None,
            app_version: "0.2.2",
        };
        assert_eq!(
            serde_json::to_value(&payload).unwrap(),
            serde_json::json!({
                "rule": "placeholder-assertion",
                "ruleTitle": "Vérification qui ne teste rien",
                "severity": "warning",
                "file": "src/app.test.ts",
                "line": 12,
                "comment": "Voulu",
                "excerpt": null,
                "excerptStartLine": null,
                "appVersion": "0.2.2"
            })
        );
    }

    #[test]
    fn the_excerpt_surrounds_the_reported_line() {
        let text = (1..=20)
            .map(|n| format!("ligne {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let excerpt = excerpt_from(&text, 10).unwrap();
        let numbers: Vec<u32> = excerpt.lines.iter().map(|(n, _)| *n).collect();
        assert_eq!(numbers, vec![7, 8, 9, 10, 11, 12, 13]);
        assert_eq!(excerpt.lines[3].1, "ligne 10");
    }

    #[test]
    fn the_excerpt_is_clipped_at_the_edges_of_the_file() {
        let excerpt = excerpt_from("a\nb\nc", 1).unwrap();
        let numbers: Vec<u32> = excerpt.lines.iter().map(|(n, _)| *n).collect();
        assert_eq!(numbers, vec![1, 2, 3]);
    }

    #[test]
    fn a_line_past_the_end_of_the_file_gives_no_excerpt() {
        assert!(excerpt_from("a\nb", 9).is_none());
    }
}

//! « Vue d'ensemble » : l'annonce du héraut, puis les projets rangés en trois
//! colonnes selon leur état (urgent, à améliorer, en bonne santé). Chaque carte
//! montre la note, l'évolution, la courbe des notes, les critiques, problèmes et
//! alertes CVE ; un clic ouvre le projet.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, Div, FontWeight, Hsla, PathBuilder, canvas, div, point,
    prelude::*, pulsating_between, px,
};

use super::app::HeraldApp;
use super::components::{Button, ButtonVariant, IconName, ProgressBar, grade_color, herald_banner};
use super::home::{count_label, surface};
use crate::model::{AnalysisReport, Grade, Project, Severity};
use crate::theme::{Theme, fonts, metrics};

/// Courbe de l'historique dans une carte.
const SPARK_W: gpui::Pixels = px(90.);
const SPARK_H: gpui::Pixels = px(26.);
/// Analyses tracées au plus dans la courbe d'une carte.
const SPARK_POINTS: usize = 12;
const GRADE_SIZE: gpui::Pixels = px(26.);
/// En dessous : urgent ; à partir de : en bonne santé (A- ou mieux).
const URGENT_BELOW: u8 = 60;
const HEALTHY_FROM: u8 = 90;

/// Colonne d'un projet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Urgent,
    Improve,
    Healthy,
}

/// État d'un projet analysé : urgent sous 60, ou avec au moins deux critiques
/// tant qu'il n'est pas à A- ; en bonne santé à partir de 90 ; sinon à améliorer.
fn status(score: u8, critical: u32) -> Status {
    if score < URGENT_BELOW || (critical >= 2 && score < HEALTHY_FROM) {
        Status::Urgent
    } else if score >= HEALTHY_FROM {
        Status::Healthy
    } else {
        Status::Improve
    }
}

/// Un projet analysé, pour l'annonce du héraut.
struct ProjectLine<'a> {
    name: &'a str,
    score: u8,
    /// Écart avec l'analyse précédente.
    delta: Option<i16>,
    critical: u32,
}

/// Annonce du héraut sur l'ensemble des projets : combien sont en urgence (le
/// plus critique en tête), combien vont bien, et la plus forte progression.
fn status_announcement(lines: &[ProjectLine]) -> (String, String) {
    if lines.is_empty() {
        return (
            "Lancez une première analyse pour que le héraut fasse son rapport.".to_owned(),
            "Chaque projet analysé trouve sa place dans l'une des trois colonnes.".to_owned(),
        );
    }
    let urgent: Vec<&ProjectLine> = lines
        .iter()
        .filter(|l| status(l.score, l.critical) == Status::Urgent)
        .collect();
    let healthy = lines
        .iter()
        .filter(|l| status(l.score, l.critical) == Status::Healthy)
        .count();
    let headline = match urgent.len() {
        0 if healthy == lines.len() => "Tous vos projets sont en bonne santé.".to_owned(),
        0 => "Aucun projet n'est en urgence.".to_owned(),
        1 => "1 projet demande une intervention urgente.".to_owned(),
        n => format!("{n} projets demandent une intervention urgente."),
    };

    let mut detail: Vec<String> = Vec::new();
    if let Some(worst) = urgent.iter().max_by_key(|l| (l.critical, 100 - l.score)) {
        detail.push(if worst.critical > 0 {
            format!(
                "{} en tête avec {}.",
                worst.name,
                count_label(worst.critical, "problème critique", "problèmes critiques")
            )
        } else {
            format!("{} est à {}.", worst.name, Grade::from_score(worst.score))
        });
    }
    let best = lines
        .iter()
        .filter(|l| l.delta.is_some_and(|d| d > 0))
        .max_by_key(|l| l.delta.unwrap_or(0));
    match (healthy, best) {
        (0, Some(best)) => detail.push(format!(
            "{} gagne {}.",
            best.name,
            count_label(
                u32::from(best.delta.unwrap_or(0).unsigned_abs()),
                "point",
                "points"
            )
        )),
        (0, None) => {}
        (n, Some(best)) => detail.push(format!(
            "{} en bonne santé, dont {} qui gagne {}.",
            count_label(n as u32, "projet", "projets"),
            best.name,
            count_label(
                u32::from(best.delta.unwrap_or(0).unsigned_abs()),
                "point",
                "points"
            )
        )),
        (n, None) => detail.push(format!(
            "{} en bonne santé.",
            count_label(n as u32, "projet", "projets")
        )),
    }
    (headline, detail.join(" "))
}

/// Problèmes critiques encore ouverts d'un rapport.
fn open_critical(report: &AnalysisReport) -> u32 {
    report
        .issues
        .iter()
        .enumerate()
        .filter(|(ix, issue)| {
            issue.severity == Severity::Critical && !issue.review && !report.is_resolved(*ix)
        })
        .count() as u32
}

/// Courbe dorée des notes d'un projet (ordre chronologique).
pub(super) fn sparkline(scores: Vec<u8>, theme: Theme) -> impl IntoElement {
    let line = theme.halo;
    div().flex_none().w(SPARK_W).h(SPARK_H).child(
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                if scores.len() < 2 {
                    return;
                }
                let low = f32::from(*scores.iter().min().unwrap_or(&0)) - 3.;
                let high = f32::from(*scores.iter().max().unwrap_or(&100)) + 3.;
                let range = (high - low).max(1.);
                let step = (bounds.size.width - px(6.)) / (scores.len() - 1) as f32;
                let at = |ix: usize, score: u8| {
                    point(
                        bounds.left() + px(3.) + step * ix as f32,
                        bounds.bottom()
                            - px(3.)
                            - (bounds.size.height - px(6.)) * ((f32::from(score) - low) / range),
                    )
                };
                let mut path = PathBuilder::stroke(px(2.));
                for (ix, &score) in scores.iter().enumerate() {
                    if ix == 0 {
                        path.move_to(at(ix, score));
                    } else {
                        path.line_to(at(ix, score));
                    }
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, line);
                }
                let last = at(scores.len() - 1, scores[scores.len() - 1]);
                window.paint_quad(
                    gpui::fill(
                        gpui::Bounds::new(
                            point(last.x - px(3.), last.y - px(3.)),
                            gpui::size(px(6.), px(6.)),
                        ),
                        line,
                    )
                    .corner_radii(px(3.)),
                );
            },
        )
        .size_full(),
    )
}

impl HeraldApp {
    /// Ouvre la page d'un projet.
    pub(super) fn open_project(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.projects.len() {
            return;
        }
        if ix != self.selected_project {
            self.selected_project = ix;
            self.reset_selection();
        }
        self.screen = self.project_screen();
        cx.notify();
    }

    pub(super) fn render_overview(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        if self.projects.is_empty() {
            return self.render_project_page(theme, cx);
        }

        let lines: Vec<ProjectLine> = self
            .projects
            .iter()
            .filter_map(|project| {
                let report = project.report.as_ref()?;
                Some(ProjectLine {
                    name: &project.name,
                    score: report.summary.score,
                    delta: report.summary.delta(),
                    critical: open_critical(report),
                })
            })
            .collect();
        let (headline, detail) = status_announcement(&lines);

        // Projets par colonne ; ceux pas encore analysés vont « à améliorer ».
        let mut columns: [Vec<usize>; 3] = Default::default();
        for (ix, project) in self.projects.iter().enumerate() {
            let column = match &project.report {
                Some(report) => status(report.summary.score, open_critical(report)),
                None => Status::Improve,
            };
            columns[column as usize].push(ix);
        }
        // Les plus faibles d'abord dans chaque colonne.
        for column in &mut columns {
            column.sort_by_key(|&ix| {
                self.projects[ix]
                    .report
                    .as_ref()
                    .map_or(0, |r| r.summary.score)
            });
        }

        let specs = [
            (
                Status::Urgent,
                "Urgent",
                "Problèmes critiques à corriger",
                theme.danger,
            ),
            (
                Status::Improve,
                "À améliorer",
                "La note peut encore monter",
                theme.warning,
            ),
            (
                Status::Healthy,
                "En bonne santé",
                "A- ou mieux",
                theme.success,
            ),
        ];
        let board =
            div()
                .flex()
                .items_start()
                .gap(metrics::SPACE_4)
                .children(specs.into_iter().map(|(status, title, subtitle, tone)| {
                    self.status_column(&columns[status as usize], title, subtitle, tone, theme, cx)
                }));

        div()
            .flex()
            .flex_col()
            .gap(metrics::SPACE_5)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(metrics::TEXT_XL)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Vue d'ensemble"),
                    )
                    .child(
                        Button::new("overview-add-project", "Ajouter un projet", theme)
                            .icon(IconName::Plus)
                            .on_click(
                                cx.listener(|app, _, window, cx| app.open_new_project(window, cx)),
                            ),
                    ),
            )
            .child(herald_banner(
                "Le héraut annonce",
                headline,
                detail,
                None,
                theme,
            ))
            .child(board)
            .into_any_element()
    }

    /// Une colonne : titre, nombre, sous-titre, puis ses cartes.
    fn status_column(
        &self,
        projects: &[usize],
        title: &'static str,
        subtitle: &'static str,
        tone: Hsla,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(metrics::SPACE_3)
            .p(metrics::SPACE_3)
            .rounded(metrics::RADIUS_LG)
            .border_1()
            .border_color(tone.opacity(0.22))
            .bg(tone.opacity(0.05))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(metrics::SPACE_2)
                            .child(div().size(metrics::DOT).rounded_full().bg(tone))
                            .child(
                                div()
                                    .text_size(metrics::TEXT_MD)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(metrics::TEXT_SM)
                                    .text_color(theme.ink_muted)
                                    .child(projects.len().to_string()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(metrics::TEXT_XS)
                            .text_color(theme.ink_muted)
                            .child(subtitle),
                    ),
            )
            .when(projects.is_empty(), |this| {
                this.child(
                    div()
                        .py(metrics::SPACE_3)
                        .text_size(metrics::TEXT_SM)
                        .text_color(theme.ink_soft)
                        .child("Aucun projet ici."),
                )
            })
            .children(
                projects
                    .iter()
                    .map(|&ix| self.status_card(ix, &self.projects[ix], theme, cx)),
            )
    }

    /// Carte d'un projet dans sa colonne.
    fn status_card(
        &self,
        ix: usize,
        project: &Project,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let analyzing = self.is_analyzing(&project.path);
        let alerts = self
            .alerts
            .iter()
            .filter(|alert| alert.project_path == project.path)
            .count();

        let mut card = surface(theme)
            .id(("project-card", ix))
            .flex()
            .flex_col()
            .gap(metrics::SPACE_2)
            .px(metrics::SPACE_4)
            .py(metrics::SPACE_3)
            .border_color(if ix == self.selected_project {
                theme.accent
            } else {
                theme.border
            })
            .cursor_pointer()
            .hover(|style| style.border_color(theme.ink_soft))
            .on_click(cx.listener(move |app, _, _, cx| app.open_project(ix, cx)));

        let name = div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(metrics::TEXT_MD)
            .font_weight(FontWeight::SEMIBOLD)
            .child(project.name.clone());

        if let Some(run) = self.run_snapshot(&project.path) {
            card = card
                .child(div().flex().child(name))
                .child(running_body(&run, theme));
        } else if let Some(report) = &project.report {
            let grade = report.summary.grade();
            let critical = open_critical(report);
            let history: Vec<u8> = self
                .score_history
                .get(&project.path)
                .map(|entries| {
                    entries
                        .iter()
                        .take(SPARK_POINTS)
                        .rev()
                        .map(|e| e.score)
                        .collect()
                })
                .unwrap_or_default();
            let mut facts: Vec<String> = Vec::new();
            if critical > 0 {
                facts.push(count_label(critical, "critique", "critiques"));
            }
            facts.push(count_label(
                report.issues.len() as u32,
                "problème",
                "problèmes",
            ));
            if alerts > 0 {
                facts.push(format!("{alerts} CVE"));
            }
            card = card
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(metrics::SPACE_2)
                        .child(name)
                        .child(
                            div()
                                .flex_none()
                                .font_family(fonts::DISPLAY)
                                .font_weight(FontWeight::BOLD)
                                .text_size(GRADE_SIZE)
                                .line_height(GRADE_SIZE)
                                .text_color(grade_color(grade, &theme))
                                .child(grade.label()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(metrics::SPACE_2)
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap(metrics::SPACE_2)
                                .text_size(metrics::TEXT_SM)
                                .text_color(theme.ink_muted)
                                .child(format!("{} / 100", report.summary.score))
                                .children(report.summary.delta().filter(|d| *d != 0).map(|d| {
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(if d > 0 {
                                            theme.success
                                        } else {
                                            theme.danger
                                        })
                                        .child(format!("{d:+}"))
                                })),
                        )
                        .when(history.len() > 1, |this| {
                            this.child(sparkline(history, theme))
                        }),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(metrics::TEXT_XS)
                        .text_color(if critical > 0 || alerts > 0 {
                            theme.danger
                        } else {
                            theme.ink_muted
                        })
                        .child(facts.join(" · ")),
                )
                .child(
                    div()
                        .text_size(metrics::TEXT_XS)
                        .text_color(theme.ink_soft)
                        .child(format!("analysé {}", report.summary.analyzed_at)),
                );
        } else {
            card = card.child(div().flex().child(name)).child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(metrics::SPACE_2)
                    .child(
                        div()
                            .text_size(metrics::TEXT_SM)
                            .text_color(theme.ink_muted)
                            .child(if analyzing {
                                "Analyse en cours…"
                            } else {
                                "Pas encore analysé"
                            }),
                    )
                    .when(!analyzing, |this| {
                        this.child(
                            Button::new(("card-analyze", ix), "Lancer l'analyse", theme)
                                .variant(ButtonVariant::Primary)
                                .icon(IconName::Play)
                                .on_click(cx.listener(move |app, _, _, cx| {
                                    cx.stop_propagation();
                                    app.analyze_project_at(ix, false, cx);
                                })),
                        )
                    }),
            );
        }

        if analyzing {
            // Bordure dorée qui « respire » pendant l'analyse.
            let accent = theme.accent;
            card.with_animation(
                ("project-card-pulse", ix),
                Animation::new(Duration::from_millis(1_600))
                    .repeat()
                    .with_easing(pulsating_between(0.25, 1.)),
                move |card, delta| card.border_color(accent.opacity(delta)),
            )
            .into_any_element()
        } else {
            card.into_any_element()
        }
    }
}

/// Carte pendant l'analyse : pourcentage, étape, barre et fichier lu.
fn running_body(run: &super::analysis_run::RunSnapshot, theme: Theme) -> AnyElement {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .justify_end()
        .gap(metrics::SPACE_2)
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(px(2.))
                .child(
                    div()
                        .font_family(fonts::DISPLAY)
                        .font_weight(FontWeight::BOLD)
                        .text_size(GRADE_SIZE)
                        .line_height(GRADE_SIZE)
                        .text_color(theme.accent_text)
                        .child(run.percent.to_string()),
                )
                .child(
                    div()
                        .text_size(metrics::TEXT_LG)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.accent_text)
                        .child("%"),
                ),
        )
        .child(ProgressBar::new(run.percent as f32 / 100., theme))
        .child(
            div()
                .truncate()
                .text_size(metrics::TEXT_SM)
                .child(run.step.clone()),
        )
        .child(
            div()
                .truncate()
                .font_family(fonts::MONO)
                .text_size(metrics::TEXT_XS)
                .text_color(theme.ink_soft)
                .child(
                    run.file
                        .clone()
                        .unwrap_or_else(|| "Analyse locale · aucun fichier envoyé".into()),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{ProjectLine, Status, status, status_announcement};

    fn line(name: &str, score: u8, delta: Option<i16>, critical: u32) -> ProjectLine<'_> {
        ProjectLine {
            name,
            score,
            delta,
            critical,
        }
    }

    #[test]
    fn projects_are_sorted_into_three_columns() {
        assert_eq!(status(5, 46), Status::Urgent);
        assert_eq!(status(73, 2), Status::Urgent);
        assert_eq!(status(75, 1), Status::Improve);
        assert_eq!(status(81, 0), Status::Improve);
        // A avec deux critiques : en bonne santé malgré tout.
        assert_eq!(status(95, 2), Status::Healthy);
        assert_eq!(status(90, 0), Status::Healthy);
    }

    #[test]
    fn the_herald_announces_urgent_projects_first() {
        let lines = [
            line("Cutctus", 95, Some(6), 2),
            line("heraldapp-rs", 75, None, 1),
            line("rustmotion", 73, Some(-2), 2),
            line("demo-repo-3", 5, None, 46),
            line("devinelaville", 91, None, 0),
            line("demo-patterns", 92, Some(0), 0),
        ];
        let (headline, detail) = status_announcement(&lines);
        assert_eq!(headline, "2 projets demandent une intervention urgente.");
        assert_eq!(
            detail,
            "demo-repo-3 en tête avec 46 problèmes critiques. \
             3 projets en bonne santé, dont Cutctus qui gagne 6 points."
        );
    }

    #[test]
    fn calm_and_empty_portfolios() {
        let (headline, _) = status_announcement(&[line("demo", 92, None, 0)]);
        assert_eq!(headline, "Tous vos projets sont en bonne santé.");
        let (headline, detail) = status_announcement(&[line("demo", 80, Some(3), 0)]);
        assert_eq!(headline, "Aucun projet n'est en urgence.");
        assert_eq!(detail, "demo gagne 3 points.");
        let (headline, _) = status_announcement(&[]);
        assert!(headline.starts_with("Lancez une première analyse"));
    }
}

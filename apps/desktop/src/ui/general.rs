//! Onglet « Général » d'un projet : l'annonce du héraut, puis quatre axes en
//! grandes cartes (Sécurité, Qualité, Architecture, Dépendances), chacune avec
//! sa note, ce qui s'y passe et la courbe de ses dernières analyses. L'axe
//! Architecture regroupe Pattern, Versioning, DevOps et Tests. Un clic sur une
//! carte ouvre l'onglet de la catégorie.
//!
//! La carte du code (dossiers en treemap, aire selon le nombre de fichiers,
//! couleur selon les problèmes par fichier) est dans l'onglet Code.

use std::path::Path;

use gpui::{
    AnyElement, Div, FontWeight, Hsla, PathBuilder, Pixels, SharedString, canvas, div, point,
    prelude::*, px, relative, svg,
};

use super::app::{HeraldApp, WorkspaceView};
use super::components::{Button, ProgressBar, grade_color, herald_banner, herald_banner_with};
use super::home::{count_label, surface, thousands};
use crate::model::code_map::{self, Folder, Rect};
use crate::model::{AnalysisReport, Category, CategoryScore, Grade, Severity};
use crate::theme::{Theme, fonts, metrics};

/// Hauteur de la carte et son rapport largeur sur hauteur (pour la disposition).
const MAP_H: Pixels = px(400.);
const MAP_ASPECT: f32 = 2.0;
/// Écart entre deux dossiers de la carte.
const TILE_GAP: Pixels = px(3.);
/// Aire (fraction de la carte) sous laquelle un dossier ne montre que son nom.
const SMALL_TILE: f32 = 0.035;
/// Analyses tracées au plus dans la courbe d'un axe, et sa hauteur.
const TREND_POINTS: usize = 5;
const TREND_H: Pixels = px(70.);
/// Largeur de fenêtre sous laquelle les axes passent sur une colonne.
const ONE_COLUMN: Pixels = px(1080.);

/// Icône de chaque catégorie (chemin d'asset).
pub(super) fn category_icon(category: Category) -> &'static str {
    match category {
        Category::Security => "icons/category-security.svg",
        Category::Quality => "icons/category-quality.svg",
        Category::Git => "icons/git-branch.svg",
        Category::Devops => "icons/category-devops.svg",
        Category::Pattern => "icons/category-pattern.svg",
        Category::Dependencies => "icons/category-dependencies.svg",
        Category::Tests => "icons/category-tests.svg",
    }
}

impl HeraldApp {
    /// Onglet Général : l'en-tête, la carte d'analyse en cours s'il y en a une,
    /// l'annonce du héraut, puis les quatre axes.
    pub(super) fn render_general_page(
        &self,
        report: &AnalysisReport,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(project) = self.project() else {
            return div().into_any_element();
        };
        let page = div()
            .id("general-view")
            .size_full()
            .overflow_y_scroll()
            .px(metrics::SPACE_8)
            .py(metrics::SPACE_6)
            .flex()
            .flex_col()
            .gap(metrics::SPACE_5)
            .child(self.workspace_header(theme, cx))
            // Seul un échec garde sa carte : la progression et le résultat
            // passent par le bandeau du héraut.
            .when(self.run_failed(), |this| {
                this.children(self.render_analysis_card(theme, cx))
            });

        let history = self.score_history.get(&project.path);
        let previous = history
            .and_then(|entries| entries.get(1))
            .map(|entry| entry.score);
        let critical: Vec<(&'static str, u32)> = AXES
            .iter()
            .map(|axis| (axis.name, axis_counts(report, axis).1))
            .filter(|&(_, n)| n > 0)
            .collect();
        let (headline, detail) = announcement(
            &project.name,
            report.summary.score,
            previous,
            &critical,
            report
                .next_grade()
                .map(|next| (next.grade.label(), next.points_needed)),
        );
        let banner = match self.run_progress() {
            Some(run) => herald_banner_with(
                "Le héraut analyse",
                run.title,
                format!("{} · {}", run.facts, run.detail),
                Some(ProgressBar::new(run.percent as f32 / 100., theme).into_any_element()),
                Some(
                    div()
                        .flex_none()
                        .font_family(fonts::MONO)
                        .text_size(metrics::TEXT_MD)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("{} %", run.percent))
                        .into_any_element(),
                ),
                theme,
            ),
            None => herald_banner(
                "Le héraut annonce",
                headline,
                detail,
                Some(
                    div()
                        .flex()
                        .gap(metrics::SPACE_2)
                        .child(Button::new("general-plan", "Voir le plan", theme).on_click(
                            cx.listener(|app, _, window, cx| {
                                app.set_workspace_view(WorkspaceView::List, window, cx)
                            }),
                        ))
                        .into_any_element(),
                ),
                theme,
            ),
        };

        let alerts = self
            .alerts
            .iter()
            .filter(|alert| alert.project_path == project.path)
            .count() as u32;
        let cards: Vec<AnyElement> = AXES
            .iter()
            .enumerate()
            .map(|(ix, axis)| {
                let trend: Vec<u8> = history
                    .map(|entries| {
                        entries
                            .iter()
                            .take(TREND_POINTS)
                            .rev()
                            .filter_map(|entry| axis_score_of(&entry.categories, axis))
                            .collect()
                    })
                    .unwrap_or_default();
                self.axis_card(ix, axis, report, trend, alerts, theme, cx)
            })
            .collect();
        let mut cards = cards.into_iter();
        let row = |a: Option<AnyElement>, b: Option<AnyElement>| {
            div()
                .flex_none()
                .flex()
                .gap(metrics::SPACE_4)
                .children(a)
                .children(b)
        };
        // Fenêtre étroite : une carte par ligne.
        let rows: Vec<Div> = if self.viewport_width < ONE_COLUMN {
            cards.map(|card| row(Some(card), None)).collect()
        } else {
            vec![
                row(cards.next(), cards.next()),
                row(cards.next(), cards.next()),
            ]
        };

        page.child(div().flex_none().child(banner))
            .children(rows)
            .into_any_element()
    }

    /// Carte d'un axe : icône, nom, note ; ce qui s'y passe ; courbe des
    /// dernières analyses ; les catégories regroupées (Architecture).
    #[allow(clippy::too_many_arguments)]
    fn axis_card(
        &self,
        ix: usize,
        axis: &'static Axis,
        report: &AnalysisReport,
        trend: Vec<u8>,
        alerts: u32,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let score = axis_score_of(&report.categories, axis);
        let grade = score.map(Grade::from_score);
        let color = grade.map_or(theme.ink_soft, |g| grade_color(g, &theme));
        let line = axis_line(report, axis, alerts);
        let target = axis.categories[0];

        let header = div()
            .flex()
            .items_center()
            .gap(metrics::SPACE_3)
            .child(
                div()
                    .flex_none()
                    .size(px(40.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(metrics::RADIUS)
                    .bg(theme.bg_page)
                    .border_1()
                    .border_color(theme.border_soft)
                    .child(
                        svg()
                            .path(category_icon(axis.categories[0]))
                            .size(px(20.))
                            .text_color(color),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(metrics::TEXT_LG)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(axis.name),
            )
            .child(
                div()
                    .flex_none()
                    .font_family(fonts::DISPLAY)
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(44.))
                    .line_height(px(46.))
                    .text_color(color)
                    .child(grade.map_or("—", |g| g.label())),
            );

        let facts = div()
            .flex()
            .flex_wrap()
            .gap(metrics::SPACE_1)
            .text_size(metrics::TEXT_SM)
            .children(line.into_iter().enumerate().map(|(i, (text, alarm))| {
                div()
                    .text_color(if alarm { theme.danger } else { theme.ink_muted })
                    .child(if i == 0 { text } else { format!("· {text}") })
            }));

        let chart = div()
            .flex()
            .items_end()
            .justify_between()
            .gap(metrics::SPACE_4)
            .child(
                div()
                    .flex_1()
                    .min_w(px(80.))
                    .max_w(px(360.))
                    .h(TREND_H)
                    .child(trend_line(trend.clone(), theme.success)),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(metrics::TEXT_XS)
                    .text_color(theme.ink_muted)
                    .child(match trend.len() {
                        0 | 1 => "Première analyse".to_owned(),
                        n => format!("{n} dernières analyses"),
                    }),
            );

        let chips = (axis.categories.len() > 1).then(|| {
            div()
                .flex()
                .flex_wrap()
                .gap(metrics::SPACE_2)
                .children(axis.categories.iter().map(|&category| {
                    let s = category_score(report, category).filter(|s| s.applicable);
                    div()
                        .flex()
                        .items_center()
                        .gap(metrics::SPACE_1)
                        .px(metrics::SPACE_3)
                        .py(px(3.))
                        .rounded_full()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.bg_page)
                        .text_size(metrics::TEXT_XS)
                        .child(category.label())
                        .child(
                            div()
                                .font_family(fonts::DISPLAY)
                                .font_weight(FontWeight::BOLD)
                                .text_color(tone(s, theme))
                                .child(s.map_or("—", |s| s.grade().label())),
                        )
                }))
        });

        surface(theme)
            .id(SharedString::from(format!("axis-{ix}")))
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(metrics::SPACE_4)
            .p(metrics::SPACE_5)
            .cursor_pointer()
            .hover(|style| style.border_color(theme.ink_soft))
            .on_click(cx.listener(move |app, _, window, cx| open_category(app, target, window, cx)))
            .child(header)
            .child(facts)
            .child(chart)
            .children(chips)
            .into_any_element()
    }

    /// Fichiers analysés du projet affiché, relus du disque une fois par analyse.
    fn analyzed_paths(&self, root: &Path, analyzed_at: u64) -> Vec<String> {
        let mut cache = self.code_maps.borrow_mut();
        if let Some((at, paths)) = cache.get(root)
            && *at == analyzed_at
        {
            return paths.clone();
        }
        let paths = crate::engine::collect::analyzed_paths(root);
        cache.insert(root.to_path_buf(), (analyzed_at, paths.clone()));
        paths
    }

    /// Carte du code : un rectangle par dossier.
    pub(super) fn code_map_card(
        &self,
        report: &AnalysisReport,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        let card = surface(theme)
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(metrics::SPACE_3)
            .p(metrics::SPACE_4)
            .child(
                div()
                    .text_size(metrics::TEXT_SM)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Carte du code"),
            );
        let Some(project) = self.project() else {
            return card;
        };
        let paths = self.analyzed_paths(&project.path, report.summary.analyzed_at_unix);
        let folders = code_map::folders(&paths, report);
        if folders.is_empty() {
            return card.child(
                div()
                    .py(metrics::SPACE_6)
                    .text_size(metrics::TEXT_SM)
                    .text_color(theme.ink_muted)
                    .child("Le dossier du projet n'est plus accessible sur cet ordinateur."),
            );
        }
        let weights: Vec<f32> = folders.iter().map(|f| f.files as f32).collect();
        let rects = code_map::squarify(&weights, MAP_ASPECT);
        let tiles = folders
            .iter()
            .zip(rects)
            .enumerate()
            .map(|(ix, (folder, rect))| folder_tile(ix, folder, rect, theme, cx));

        card.child(div().relative().w_full().h(MAP_H).children(tiles))
            .child(legend(theme))
    }
}

/// Un axe de la page : une ou plusieurs catégories notées ensemble.
struct Axis {
    name: &'static str,
    categories: &'static [Category],
}

const AXES: [Axis; 4] = [
    Axis {
        name: "Sécurité",
        categories: &[Category::Security],
    },
    Axis {
        name: "Qualité",
        categories: &[Category::Quality],
    },
    Axis {
        name: "Architecture",
        categories: &[
            Category::Pattern,
            Category::Git,
            Category::Devops,
            Category::Tests,
        ],
    },
    Axis {
        name: "Dépendances",
        categories: &[Category::Dependencies],
    },
];

/// Note d'un axe : la moyenne de ses catégories examinées (aucune : `None`).
fn axis_score_of(categories: &[CategoryScore], axis: &Axis) -> Option<u8> {
    let scores: Vec<u32> = categories
        .iter()
        .filter(|s| s.applicable && axis.categories.contains(&s.category))
        .map(|s| u32::from(s.score))
        .collect();
    (!scores.is_empty())
        .then(|| (scores.iter().sum::<u32>() as f32 / scores.len() as f32).round() as u8)
}

/// Problèmes ouverts de l'axe, et parmi eux les critiques.
fn axis_counts(report: &AnalysisReport, axis: &Axis) -> (u32, u32) {
    axis.categories
        .iter()
        .map(|&c| open_counts(report, c))
        .fold((0, 0), |(o, c), (a, b)| (o + a, c + b))
}

/// Règle la plus fréquente parmi les problèmes ouverts de l'axe.
fn top_rule(report: &AnalysisReport, axis: &Axis) -> Option<(String, u32)> {
    let mut counts: Vec<(&str, u32)> = Vec::new();
    for (ix, issue) in report.issues.iter().enumerate() {
        if !axis.categories.contains(&issue.category) || issue.review || report.is_resolved(ix) {
            continue;
        }
        match counts.iter_mut().find(|(t, _)| *t == issue.title.as_ref()) {
            Some(entry) => entry.1 += 1,
            None => counts.push((issue.title.as_ref(), 1)),
        }
    }
    counts
        .into_iter()
        .max_by_key(|&(_, n)| n)
        .map(|(t, n)| (t.to_owned(), n))
}

/// Ligne de faits d'une carte : (texte, alarme). L'axe Architecture détaille
/// ses catégories ; Dépendances ajoute les alertes CVE.
fn axis_line(report: &AnalysisReport, axis: &Axis, alerts: u32) -> Vec<(String, bool)> {
    if axis.categories.len() > 1 {
        let mut examined: Vec<String> = Vec::new();
        let mut idle: Vec<&str> = Vec::new();
        for &category in axis.categories {
            match category_score(report, category).filter(|s| s.applicable) {
                Some(s) => examined.push(format!("{} {}", category.label(), s.score)),
                None => idle.push(category.label()),
            }
        }
        let mut out: Vec<(String, bool)> = examined.into_iter().map(|t| (t, false)).collect();
        if !idle.is_empty() {
            let names = match idle.as_slice() {
                [one] => (*one).to_owned(),
                [rest @ .., last] => format!("{} et {last}", rest.join(", ")),
                [] => String::new(),
            };
            out.push((format!("{names} : rien à examiner"), false));
        }
        return out;
    }
    let category = axis.categories[0];
    let (open, critical) = axis_counts(report, axis);
    let mut out: Vec<(String, bool)> = Vec::new();
    if category_score(report, category).is_none_or(|s| !s.applicable) && open == 0 {
        out.push(("Rien à examiner".to_owned(), false));
    } else if open == 0 {
        out.push(("Aucun problème".to_owned(), false));
    } else {
        out.push((count_label(open, "problème", "problèmes"), false));
        if critical > 0 {
            out.push((count_label(critical, "critique", "critiques"), true));
        }
    }
    if category == Category::Dependencies {
        if alerts > 0 {
            out.push((count_label(alerts, "alerte CVE", "alertes CVE"), false));
        }
    } else if let Some((rule, n)) = top_rule(report, axis).filter(|&(_, n)| n > 1) {
        out.push((format!("{rule} ×{}", thousands(n)), false));
    }
    out
}

/// Annonce du héraut : la note et son évolution, puis ce qui reste critique
/// (ou le palier suivant).
fn announcement(
    name: &str,
    score: u8,
    previous: Option<u8>,
    critical: &[(&str, u32)],
    next: Option<(&str, u8)>,
) -> (String, String) {
    let grade = Grade::from_score(score).label();
    let delta = previous.map(|p| i32::from(score) - i32::from(p));
    let points = |d: i32| {
        if d > 0 {
            format!("+{}", count_label(d as u32, "point", "points"))
        } else {
            format!("−{}", count_label(d.unsigned_abs(), "point", "points"))
        }
    };
    let headline = match (previous, delta) {
        (None, _) => format!("{name} obtient {grade} à sa première analyse."),
        (Some(p), Some(0)) if Grade::from_score(p).label() == grade => {
            format!("{name} reste à {grade}, note stable.")
        }
        (Some(p), Some(d)) if Grade::from_score(p).label() == grade => {
            format!(
                "{name} reste à {grade}, {} depuis l'analyse précédente.",
                points(d)
            )
        }
        (Some(_), Some(d)) if d > 0 => {
            format!(
                "{name} passe à {grade}, {} depuis l'analyse précédente.",
                points(d)
            )
        }
        (Some(_), Some(d)) => {
            format!(
                "{name} descend à {grade}, {} depuis l'analyse précédente.",
                points(d)
            )
        }
        _ => String::new(),
    };
    let total: u32 = critical.iter().map(|(_, n)| n).sum();
    let detail = if total > 0 {
        let places: Vec<String> = critical.iter().map(|(a, _)| a.to_lowercase()).collect();
        let places = match places.as_slice() {
            [one] => one.clone(),
            [rest @ .., last] => format!("{} et {last}", rest.join(", ")),
            [] => String::new(),
        };
        format!(
            "{} {}, en {places}.",
            count_label(total, "problème critique", "problèmes critiques"),
            if total > 1 { "restent" } else { "reste" }
        )
    } else {
        match next {
            Some((g, n)) => format!(
                "Aucun problème critique. Encore {} pour atteindre {g}.",
                count_label(u32::from(n), "point", "points")
            ),
            None => "Aucun problème critique : la meilleure note est atteinte.".to_owned(),
        }
    };
    (headline, detail)
}

/// Courbe d'un axe : les notes des dernières analyses, à l'échelle de leur
/// écart (une note stable reste une ligne droite).
fn trend_line(scores: Vec<u8>, color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            if scores.is_empty() {
                return;
            }
            let low = f32::from(*scores.iter().min().unwrap_or(&0)) - 4.;
            let high = f32::from(*scores.iter().max().unwrap_or(&100)) + 2.;
            let range = (high - low).max(1.);
            let step = if scores.len() > 1 {
                (bounds.size.width - px(4.)) / (scores.len() - 1) as f32
            } else {
                px(0.)
            };
            let at = |ix: usize, score: u8| {
                point(
                    bounds.left() + px(2.) + step * ix as f32,
                    bounds.bottom()
                        - px(2.)
                        - (bounds.size.height - px(4.)) * ((f32::from(score) - low) / range),
                )
            };
            let mut path = PathBuilder::stroke(px(2.5));
            if scores.len() == 1 {
                path.move_to(at(0, scores[0]));
                path.line_to(point(bounds.right() - px(2.), at(0, scores[0]).y));
            } else {
                for (ix, &score) in scores.iter().enumerate() {
                    if ix == 0 {
                        path.move_to(at(ix, score));
                    } else {
                        path.line_to(at(ix, score));
                    }
                }
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        },
    )
    .size_full()
}

/// Bascule sur l'onglet qui détaille la catégorie (le Plan filtré si elle n'a
/// pas d'onglet à elle).
fn open_category(
    app: &mut HeraldApp,
    category: Category,
    window: &mut gpui::Window,
    cx: &mut Context<HeraldApp>,
) {
    match category {
        Category::Pattern => app.set_workspace_view(WorkspaceView::Patterns, window, cx),
        Category::Dependencies => app.set_workspace_view(WorkspaceView::Dependencies, window, cx),
        _ => {
            app.set_workspace_view(WorkspaceView::List, window, cx);
            app.category_filter = Some(category);
        }
    }
    cx.notify();
}

fn category_score(report: &AnalysisReport, category: Category) -> Option<&CategoryScore> {
    report
        .categories
        .iter()
        .find(|score| score.category == category)
}

/// Problèmes encore ouverts de la catégorie, et parmi eux les critiques.
fn open_counts(report: &AnalysisReport, category: Category) -> (u32, u32) {
    report
        .issues
        .iter()
        .enumerate()
        .filter(|(ix, issue)| {
            issue.category == category && !issue.review && !report.is_resolved(*ix)
        })
        .fold((0, 0), |(open, critical), (_, issue)| {
            (
                open + 1,
                critical + u32::from(issue.severity == Severity::Critical),
            )
        })
}

/// Couleur de la note ; grise pour une catégorie sans rien à examiner.
fn tone(score: Option<&CategoryScore>, theme: Theme) -> Hsla {
    match score {
        Some(score) if score.applicable => grade_color(score.grade(), &theme),
        _ => theme.ink_soft,
    }
}

/// Couleur d'un dossier : vert s'il est propre, rouge s'il a des critiques ou
/// beaucoup de problèmes par fichier, ambre sinon (plus soutenu au-delà de 1,5).
fn tile_color(folder: &Folder, theme: Theme) -> Hsla {
    let density = folder.density();
    if folder.issues == 0 {
        theme.success.opacity(0.13)
    } else if folder.critical > 0 || density > 3. {
        theme.danger.opacity(0.14)
    } else if density > 1.5 {
        theme.accent.opacity(0.30)
    } else {
        theme.accent.opacity(0.14)
    }
}

/// Un dossier de la carte, placé en fractions du conteneur.
fn folder_tile(
    ix: usize,
    folder: &Folder,
    rect: Rect,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> AnyElement {
    let small = rect.w * rect.h < SMALL_TILE;
    let meta = format!(
        "{} · {}",
        count_label(folder.files as u32, "fichier", "fichiers"),
        if folder.issues == 0 {
            "propre".to_owned()
        } else {
            count_label(folder.issues as u32, "problème", "problèmes")
        }
    );
    let open = folder.first.clone();

    let tile = div()
        .id(SharedString::from(format!("code-map-{ix}")))
        .size_full()
        .flex()
        .flex_col()
        .gap(px(2.))
        .p(if small {
            metrics::SPACE_2
        } else {
            metrics::SPACE_3
        })
        .rounded(metrics::RADIUS_LG)
        .overflow_hidden()
        .bg(tile_color(folder, theme))
        .child(
            div()
                .w_full()
                .truncate()
                .font_family(fonts::MONO)
                .font_weight(FontWeight::MEDIUM)
                .text_size(if small {
                    metrics::TEXT_XS
                } else {
                    metrics::TEXT_SM
                })
                .child(folder.name.clone()),
        )
        .when(!small, |this| {
            this.child(
                div()
                    .w_full()
                    .truncate()
                    .text_size(metrics::TEXT_XS)
                    .text_color(theme.ink_muted)
                    .child(meta),
            )
            .when(folder.critical > 0, |this| {
                this.child(
                    div()
                        .text_size(metrics::TEXT_XS)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.danger)
                        .child(count_label(folder.critical as u32, "critique", "critiques")),
                )
            })
            .children(folder.top.clone().map(|top| {
                div()
                    .w_full()
                    .truncate()
                    .text_size(metrics::TEXT_XS)
                    .text_color(theme.ink_soft)
                    .child(top)
            }))
        })
        .when_some(open, |this, (file, line)| {
            this.cursor_pointer()
                .hover(|style| style.opacity(0.85))
                .on_click(
                    cx.listener(move |app, _, window, cx| app.open_in_ide(&file, line, window, cx)),
                )
        });

    div()
        .absolute()
        .left(relative(rect.x))
        .top(relative(rect.y))
        .w(relative(rect.w))
        .h(relative(rect.h))
        .p(TILE_GAP)
        .child(tile)
        .into_any_element()
}

/// Légende de la carte.
fn legend(theme: Theme) -> Div {
    let swatch = |color: Hsla, label: &'static str| {
        div()
            .flex()
            .items_center()
            .gap(metrics::SPACE_1)
            .child(div().size(px(10.)).rounded(px(3.)).bg(color))
            .child(label)
    };
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(metrics::SPACE_4)
        .text_size(metrics::TEXT_XS)
        .text_color(theme.ink_muted)
        .child("Taille : nombre de fichiers")
        .child("Couleur : problèmes par fichier")
        .child(swatch(theme.success.opacity(0.13), "propre"))
        .child(swatch(theme.accent.opacity(0.14), "peu"))
        .child(swatch(theme.accent.opacity(0.30), "beaucoup"))
        .child(swatch(
            theme.danger.opacity(0.14),
            "critique ou très chargé",
        ))
        .child("Un clic ouvre le problème le plus grave du dossier.")
}

#[cfg(test)]
mod tests {
    use gpui::AssetSource;

    use super::{AXES, announcement, axis_score_of, category_icon};
    use crate::assets::Assets;
    use crate::model::{Category, CategoryScore};

    fn score(category: Category, score: u8, applicable: bool) -> CategoryScore {
        CategoryScore {
            category,
            score,
            issue_count: 0,
            applicable,
        }
    }

    #[test]
    fn architecture_averages_its_examined_categories() {
        let categories = [
            score(Category::Pattern, 99, true),
            score(Category::Git, 100, true),
            score(Category::Devops, 0, false),
            score(Category::Tests, 0, false),
            score(Category::Security, 40, true),
        ];
        assert_eq!(axis_score_of(&categories, &AXES[2]), Some(100));
        assert_eq!(axis_score_of(&categories, &AXES[0]), Some(40));
        assert_eq!(axis_score_of(&categories, &AXES[3]), None);
    }

    #[test]
    fn the_herald_announces_grade_and_criticals() {
        assert_eq!(
            announcement(
                "Cutctus",
                95,
                Some(89),
                &[("Dépendances", 2)],
                Some(("A+", 2))
            ),
            (
                "Cutctus passe à A, +6 points depuis l'analyse précédente.".to_owned(),
                "2 problèmes critiques restent, en dépendances.".to_owned()
            )
        );
        let (headline, detail) = announcement("demo", 94, Some(94), &[], Some(("A+", 3)));
        assert_eq!(headline, "demo reste à A, note stable.");
        assert_eq!(
            detail,
            "Aucun problème critique. Encore 3 points pour atteindre A+."
        );
        let (headline, _) = announcement(
            "demo",
            5,
            None,
            &[("Sécurité", 32), ("Dépendances", 4)],
            None,
        );
        assert_eq!(headline, "demo obtient F à sa première analyse.");
    }

    #[test]
    fn every_category_icon_is_embedded() {
        for category in Category::ALL {
            let icon = category_icon(category);
            assert!(
                Assets.load(icon).unwrap().is_some(),
                "{icon} absente de assets.rs"
            );
        }
    }
}

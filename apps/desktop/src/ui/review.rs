//! Écran « Problèmes », vue Revue de code : fichiers les plus touchés à gauche,
//! code au centre avec la fiche du problème sous la ligne fautive, problèmes
//! du fichier à droite.

use std::collections::BTreeMap;

use gpui::{AnyElement, FontWeight, Hsla, SharedString, div, prelude::*, px};

use super::analysis::{
    badge, section_caption, severity_badge, severity_color, short_points, view_switch,
};
use super::app::{HeraldApp, WorkspaceView};
use super::components::{Button, ButtonVariant, grade_color};
use super::home::count_label;
use crate::model::{AnalysisReport, Severity, rule_text};
use crate::theme::{Theme, fonts, metrics};

/// Pourquoi un relevé est « À revoir » : précision mesurée de sa règle.
fn review_note(rule: &str) -> String {
    let precision = crate::engine::local_rules::measures()
        .get(rule)
        .and_then(|measure| {
            let p = measure.precision()?;
            Some(format!(
                " : {} vrai(s) positif(s) sur {} relevés étiquetés ({} %, {} relevés sur le corpus)",
                measure.true_positives,
                measure.labeled,
                (p * 100.).round(),
                measure.total
            ))
        })
        .unwrap_or_default();
    format!(
        "Règle peu fiable{precision}. Vérifiez ce relevé avant de le corriger : \
         il ne compte pas dans la note."
    )
}

const FILES_WIDTH: gpui::Pixels = px(272.);
const LINE_NUMBER_WIDTH: gpui::Pixels = px(52.);
const LINE_HEIGHT: gpui::Pixels = px(21.);
/// Lignes affichées avant et après la ligne du problème.
const BEFORE: u32 = 8;
const AFTER: u32 = 24;
/// Largeur de lecture de la fiche posée sous la ligne.
const CARD_MAX_WIDTH: gpui::Pixels = px(760.);

/// Problèmes visibles regroupés par fichier, chaque liste triée par ligne.
fn issues_by_file(report: &AnalysisReport) -> BTreeMap<SharedString, Vec<usize>> {
    let mut files: BTreeMap<SharedString, Vec<usize>> = BTreeMap::new();
    for (ix, issue) in report.issues.iter().enumerate() {
        files.entry(issue.file.clone()).or_default().push(ix);
    }
    for list in files.values_mut() {
        list.sort_by_key(|&ix| (report.issues[ix].line.unwrap_or(0), ix));
    }
    files
}

fn worst(report: &AnalysisReport, issues: &[usize]) -> Severity {
    issues
        .iter()
        .map(|&ix| report.issues[ix].severity)
        .min()
        .unwrap_or(Severity::Info)
}

impl HeraldApp {
    /// Sélectionne un problème précis (et son groupe).
    pub(super) fn select_issue(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some((gi, occurrence)) = self.report().and_then(|report| report.group_of(ix)) {
            self.selected_group = Some(gi);
            self.selected_occurrence = occurrence;
            cx.notify();
        }
    }

    pub(super) fn render_review(
        &self,
        report: &AnalysisReport,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.project().is_none() {
            return div().into_any_element();
        }
        let files = issues_by_file(report);
        let selected = self
            .selected_issue_index()
            .or_else(|| files.values().next().and_then(|list| list.first().copied()));

        // Fichiers les plus touchés d'abord, à gravité égale.
        let mut ranked: Vec<(&SharedString, &Vec<usize>)> = files.iter().collect();
        ranked.sort_by(|a, b| {
            worst(report, a.1)
                .cmp(&worst(report, b.1))
                .then(b.1.len().cmp(&a.1.len()))
        });
        let current_file = selected.map(|ix| report.issues[ix].file.clone());

        let file_rows = ranked
            .iter()
            .enumerate()
            .map(|(row, (file, list))| {
                let active = current_file.as_ref() == Some(*file);
                let color = severity_color(worst(report, list), &theme);
                let first = list[0];
                div()
                    .id(("review-file", row))
                    .flex()
                    .items_center()
                    .gap(metrics::SPACE_2)
                    .h(px(26.))
                    .px(metrics::SPACE_2)
                    .rounded(metrics::RADIUS)
                    .cursor_pointer()
                    .when(active, |this| {
                        this.bg(theme.bg_active).font_weight(FontWeight::MEDIUM)
                    })
                    .when(!active, |this| this.hover(|style| style.bg(theme.bg_hover)))
                    .on_click(cx.listener(move |app, _, _, cx| app.select_issue(first, cx)))
                    .child(
                        div()
                            .flex_none()
                            .size(metrics::DOT)
                            .rounded_full()
                            .bg(color),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(fonts::MONO)
                            .text_size(metrics::TEXT_SM)
                            .child(short_path(file, 2)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(metrics::TEXT_XS)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(color)
                            .child(list.len().to_string()),
                    )
            })
            .collect::<Vec<_>>();

        let header = div()
            .flex()
            .items_center()
            .gap(metrics::SPACE_3)
            .px(metrics::SPACE_5)
            .py(metrics::SPACE_3)
            .border_b_1()
            .border_color(theme.border)
            .child(
                Button::new("review-back", "← Plan de correction", theme)
                    .variant(ButtonVariant::Ghost)
                    .on_click(cx.listener(|app, _, window, cx| {
                        app.set_workspace_view(WorkspaceView::List, window, cx)
                    })),
            )
            .child(
                div()
                    .font_family(fonts::DISPLAY)
                    .font_weight(FontWeight::BOLD)
                    .text_size(metrics::TEXT_XL)
                    .text_color(grade_color(report.summary.grade(), &theme))
                    .child(report.summary.grade().label()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(theme.ink_muted)
                    .child(format!(
                        "{} / 100 · {} dans {}",
                        report.summary.score,
                        count_label(report.issues.len() as u32, "problème", "problèmes"),
                        count_label(files.len() as u32, "fichier", "fichiers"),
                    )),
            )
            .child(view_switch(self.workspace_view, theme, cx));

        let center: AnyElement = match selected {
            Some(ix) => self.render_review_code(report, &files, ix, theme, cx),
            None => div()
                .p(metrics::SPACE_6)
                .text_color(theme.ink_muted)
                .child("Aucun problème à revoir.")
                .into_any_element(),
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .id("review-files")
                            .w(FILES_WIDTH)
                            .flex_none()
                            .h_full()
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .p(metrics::SPACE_2)
                            .border_r_1()
                            .border_color(theme.border)
                            .child(div().px(metrics::SPACE_2).py(metrics::SPACE_1).child(
                                section_caption("Fichiers, les plus touchés d'abord", theme),
                            ))
                            .children(file_rows),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            // Rien ne déborde sur les colonnes voisines.
                            .overflow_hidden()
                            .bg(theme.bg_surface)
                            .child(center),
                    ),
            )
            .into_any_element()
    }

    /// Code du fichier autour du problème, avec sa fiche sous la ligne.
    fn render_review_code(
        &self,
        report: &AnalysisReport,
        files: &BTreeMap<SharedString, Vec<usize>>,
        ix: usize,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let issue = &report.issues[ix];
        let list = files.get(&issue.file).cloned().unwrap_or_default();
        let pos = list.iter().position(|&other| other == ix).unwrap_or(0);
        let prev = pos.checked_sub(1).and_then(|p| list.get(p).copied());
        let next = list.get(pos + 1).copied();
        let critical = list
            .iter()
            .filter(|&&other| report.issues[other].severity == Severity::Critical)
            .count();

        let nav = |id: &'static str,
                   label: &'static str,
                   target: Option<usize>,
                   cx: &mut Context<Self>| {
            let button = Button::new(id, label, theme);
            match target {
                Some(target) => div().child(
                    button.on_click(cx.listener(move |app, _, _, cx| app.select_issue(target, cx))),
                ),
                None => div().opacity(0.4).child(button),
            }
        };

        let card = self.issue_card(report, ix, theme, cx);
        // Autres problèmes du fichier, par ligne, pour teinter leurs lignes.
        let mut marks: std::collections::HashMap<u32, Severity> = std::collections::HashMap::new();
        for &other in &list {
            if let Some(line) = report.issues[other].line {
                let entry = marks.entry(line).or_insert(report.issues[other].severity);
                *entry = (*entry).min(report.issues[other].severity);
            }
        }

        let code: AnyElement = match (report.source(&issue.file), issue.line) {
            (Some(source), Some(line)) => {
                let from = line.saturating_sub(BEFORE).max(1);
                let to = line + AFTER;
                let mut rows: Vec<AnyElement> = Vec::new();
                let mut card = Some(card);
                for (number, text) in source
                    .numbered_lines()
                    .filter(|(n, _)| (from..=to).contains(n))
                {
                    let selected = number == line;
                    let tint: Option<Hsla> = if selected {
                        Some(theme.code_highlight_bg)
                    } else {
                        marks.get(&number).map(|severity| match severity {
                            Severity::Critical => theme.code_line_critical,
                            Severity::Warning => theme.code_line_warning,
                            Severity::Info => theme.code_current_line,
                        })
                    };
                    rows.push(
                        div()
                            .flex()
                            .h(LINE_HEIGHT)
                            .items_center()
                            .whitespace_nowrap()
                            .when_some(tint, |this, tint| this.bg(tint))
                            .child(
                                div()
                                    .w(LINE_NUMBER_WIDTH)
                                    .flex_none()
                                    .flex()
                                    .justify_end()
                                    .pr(metrics::SPACE_3)
                                    .text_color(if selected {
                                        theme.code_highlight_fg
                                    } else {
                                        theme.code_line_number
                                    })
                                    .child(number.to_string()),
                            )
                            .child(
                                div()
                                    .text_color(if selected {
                                        theme.code_highlight_fg
                                    } else {
                                        theme.code_text
                                    })
                                    .child(SharedString::from(text.replace('\t', "    "))),
                            )
                            .into_any_element(),
                    );
                    if selected {
                        if let Some(card) = card.take() {
                            rows.push(
                                div()
                                    .pl(LINE_NUMBER_WIDTH)
                                    .pr(metrics::SPACE_4)
                                    .py(metrics::SPACE_2)
                                    .child(card)
                                    .into_any_element(),
                            );
                        }
                    }
                }
                div()
                    .font_family(fonts::MONO)
                    .text_size(metrics::TEXT_SM)
                    .py(metrics::SPACE_2)
                    .children(rows)
                    .into_any_element()
            }
            // Problème sans ligne (fichier entier, historique Git) : la fiche seule.
            _ => div().p(metrics::SPACE_4).child(card).into_any_element(),
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(metrics::SPACE_3)
                    .px(metrics::SPACE_4)
                    .py(metrics::SPACE_2)
                    .border_b_1()
                    .border_color(theme.border_soft)
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_family(fonts::MONO)
                            .font_weight(FontWeight::MEDIUM)
                            .child(short_path(&issue.file, 4)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(metrics::TEXT_SM)
                            .text_color(theme.ink_muted)
                            .child(format!(
                                "{}{}",
                                count_label(list.len() as u32, "problème", "problèmes"),
                                if critical > 0 {
                                    format!(
                                        " · {}",
                                        count_label(critical as u32, "critique", "critiques")
                                    )
                                } else {
                                    String::new()
                                }
                            )),
                    )
                    .child(nav("review-prev", "↑ Précédent", prev, cx))
                    .child(nav("review-next", "Suivant ↓", next, cx)),
            )
            .child(
                div()
                    .id(SharedString::from(format!("review-code-{ix}")))
                    .flex_1()
                    .min_h_0()
                    .overflow_scroll()
                    .child(code),
            )
            .into_any_element()
    }

    /// Fiche du problème, posée sous la ligne fautive.
    fn issue_card(
        &self,
        report: &AnalysisReport,
        ix: usize,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let issue = &report.issues[ix];
        let Some((gi, _)) = report.group_of(ix) else {
            return div().into_any_element();
        };
        let resolved = report.is_group_resolved(gi);
        let count = report.groups()[gi].occurrences.len();
        let can_open = issue.line.is_some() && report.source(&issue.file).is_some();
        let toggle = cx.listener(move |app, _, _, cx| app.toggle_group_resolved(gi, cx));
        let text = rule_text(&issue.rule);

        div()
            .flex()
            .flex_col()
            .gap(metrics::SPACE_2)
            .max_w(CARD_MAX_WIDTH)
            .p(metrics::SPACE_3)
            .rounded(metrics::RADIUS_LG)
            .border_1()
            .border_color(theme.border)
            .bg(theme.bg_page)
            .font_family(fonts::SANS)
            .text_size(metrics::TEXT_BASE)
            .whitespace_normal()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(metrics::SPACE_2)
                    .child(
                        div()
                            .text_size(metrics::TEXT_MD)
                            .font_weight(FontWeight::SEMIBOLD)
                            .when(resolved, |this| this.line_through())
                            .child(issue.title.clone()),
                    )
                    .child(if issue.review {
                        badge("À revoir", theme)
                            .bg(theme.track)
                            .border_color(theme.track)
                            .text_color(theme.ink_muted)
                            .into_any_element()
                    } else {
                        severity_badge(issue.severity, theme).into_any_element()
                    })
                    .child(
                        div()
                            .text_size(metrics::TEXT_SM)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.accent_text)
                            .child(format!(
                                "{} pts sur la note{}",
                                short_points(report.base_gain(gi)),
                                if count > 1 {
                                    format!(" (les {count})")
                                } else {
                                    String::new()
                                }
                            )),
                    ),
            )
            .child(
                div()
                    .font_family(fonts::MONO)
                    .text_size(metrics::TEXT_XS)
                    .text_color(theme.ink_soft)
                    .child(issue.rule.clone()),
            )
            .children(issue.review.then(|| {
                div()
                    .text_size(metrics::TEXT_SM)
                    .text_color(theme.ink_muted)
                    .child(review_note(&issue.rule))
            }))
            .children(text.map(|text| explained("Pourquoi c'est un problème", text.why, theme)))
            .children(text.map(|text| explained("Comment corriger", text.fix, theme)))
            // Détail de ce relevé (valeur mesurée, paquet, version…), tel que le moteur l'a écrit.
            .children(issue.explanation.clone().map(|detail| {
                div()
                    .text_size(metrics::TEXT_SM)
                    .text_color(theme.ink_soft)
                    .child(format!("Relevé : {detail}"))
            }))
            .children(issue.fix.clone().map(|fix| {
                div()
                    .px(metrics::SPACE_2)
                    .py(metrics::SPACE_1)
                    .rounded(metrics::RADIUS)
                    .bg(theme.code_fix_bg)
                    .text_color(theme.code_fix_fg)
                    .font_family(fonts::MONO)
                    .text_size(metrics::TEXT_SM)
                    .child(fix)
            }))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(metrics::SPACE_2)
                    .when(can_open, |this| {
                        this.child(
                            Button::new("review-edit", "Ouvrir dans l'éditeur", theme)
                                .variant(ButtonVariant::Primary)
                                .on_click(
                                    cx.listener(|app, _, window, cx| app.fix_in_ide(window, cx)),
                                ),
                        )
                    })
                    .child(if resolved {
                        Button::new(("review-unresolve", gi), "Annuler « corrigé »", theme)
                            .on_click(toggle)
                    } else if count > 1 {
                        Button::new(
                            ("review-resolve", gi),
                            format!("Marquer les {count} comme corrigés"),
                            theme,
                        )
                        .on_click(toggle)
                    } else {
                        Button::new(("review-resolve", gi), "Marquer comme corrigé", theme)
                            .on_click(toggle)
                    })
                    .when(can_open, |this| {
                        this.child(
                            Button::new("review-vscode", "VS Code", theme)
                                .variant(ButtonVariant::Ghost)
                                .on_click(cx.listener(|app, _, _, cx| app.open_in_editor(cx))),
                        )
                    }),
            )
            .into_any_element()
    }
}

/// Petit titre et son paragraphe dans la fiche d'un problème.
fn explained(label: &'static str, text: &'static str, theme: Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(
            div()
                .text_size(metrics::TEXT_XS)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.ink_soft)
                .child(label.to_uppercase()),
        )
        .child(div().text_color(theme.ink).child(text))
}

/// Garde les `keep` derniers éléments d'un chemin long : le nom du fichier
/// reste lisible (« …/Frameworks/native-bridge.js »).
fn short_path(path: &str, keep: usize) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() <= keep {
        return path.to_owned();
    }
    format!("…/{}", parts[parts.len() - keep..].join("/"))
}

#[cfg(test)]
mod tests {
    use super::short_path;

    #[test]
    fn long_paths_keep_the_file_name() {
        assert_eq!(short_path("src/app.rs", 2), "src/app.rs");
        assert_eq!(
            short_path("ios/a/b/c/native-bridge.js", 2),
            "…/c/native-bridge.js"
        );
    }
}

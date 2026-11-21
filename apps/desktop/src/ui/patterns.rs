//! Onglet « Patterns » d'un projet, en deux niveaux :
//!
//! - **la grille** : une carte courte par pattern de conception
//!   (`model::patterns`), avec son icône et son état dans le projet ;
//! - **la page d'un pattern**, ouverte d'un clic sur sa carte : pourquoi il
//!   compte, le schéma « à éviter / à faire », les astuces et tous les endroits
//!   du code qui s'en écartent (un clic ouvre le fichier à la ligne dans Code).

use gpui::{
    AnyElement, Div, FontWeight, Hsla, Pixels, SharedString, div, prelude::*, px, relative, svg,
};

use super::analysis::section_caption;
use super::app::HeraldApp;
use super::components::{Icon, IconName};
use super::home::{count_label, surface};
use crate::model::patterns::{self, Diagram, PatternStatus};
use crate::model::{AnalysisReport, rule_title};
use crate::theme::{Theme, fonts, metrics};

/// Cartes par ligne de la grille.
const GRID_COLUMNS: usize = 3;
/// Endroits listés sur la page d'un pattern ; les suivants sont seulement comptés.
const SHOWN_FINDINGS: usize = 100;
/// Pseudo-fichier des relevés de l'historique Git (pas de fichier à ouvrir).
const GIT_PSEUDO_FILE: &str = "Historique Git";

impl HeraldApp {
    /// La page du pattern ouvert, sinon la grille de tous les patterns.
    pub(super) fn render_patterns(
        &self,
        report: &AnalysisReport,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let statuses = patterns::statuses(report);
        let open = self
            .open_pattern
            .and_then(|name| statuses.iter().find(|status| status.name() == name));
        match open {
            Some(status) => self.pattern_page(status, report, theme, cx),
            None => pattern_grid(&statuses, theme, cx),
        }
    }

    /// Page d'un pattern : en-tête, schémas côte à côte, astuces, endroits.
    fn pattern_page(
        &self,
        status: &PatternStatus,
        report: &AnalysisReport,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let why = status.pattern.map_or(
            "Règles de la catégorie Pattern qui n'ont pas encore de fiche ici.",
            |pattern| pattern.why,
        );

        let back = div()
            .id("pattern-back")
            .flex()
            .items_center()
            .gap(metrics::SPACE_1)
            .mx(-metrics::SPACE_2)
            .px(metrics::SPACE_2)
            .py(metrics::SPACE_1)
            .rounded(metrics::RADIUS)
            .text_size(metrics::TEXT_SM)
            .text_color(theme.ink_muted)
            .cursor_pointer()
            .hover(|style| style.bg(theme.bg_hover).text_color(theme.ink))
            .on_click(cx.listener(|app, _, _, cx| {
                app.open_pattern = None;
                cx.notify();
            }))
            .child(Icon::new(IconName::ChevronLeft, theme.ink_muted).size(metrics::ICON_SM))
            .child("Tous les patterns");

        let header = surface(theme)
            .flex()
            .items_start()
            .gap(metrics::SPACE_4)
            .p(metrics::SPACE_5)
            .child(icon_tile(status, px(48.), px(26.), theme))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(metrics::SPACE_1)
                    .child(
                        div()
                            .text_size(metrics::TEXT_XL)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(status.name()),
                    )
                    .child(
                        div()
                            .text_size(metrics::TEXT_BASE)
                            .text_color(theme.ink_muted)
                            .child(why),
                    ),
            )
            .child(state_badge(status, theme));

        let mut page = div()
            .flex()
            .flex_col()
            .items_start()
            .gap(metrics::SPACE_4)
            .child(back)
            .child(header.w_full());

        if let Some(pattern) = status.pattern {
            page = page
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_start()
                        .gap(metrics::SPACE_4)
                        .child(diagram_panel(
                            "À éviter",
                            pattern.avoid,
                            theme.danger,
                            theme,
                        ))
                        .child(diagram_panel(
                            "À faire",
                            pattern.prefer,
                            theme.success,
                            theme,
                        )),
                )
                .child(
                    panel(theme)
                        .child(section_caption("Astuces", theme))
                        .children(pattern.tips.iter().map(|tip| {
                            div()
                                .flex()
                                .gap(metrics::SPACE_2)
                                .text_size(metrics::TEXT_BASE)
                                .child(div().flex_none().text_color(theme.ink_soft).child("–"))
                                .child(div().flex_1().min_w_0().child(*tip))
                        })),
                );
        }

        page = page.child(self.findings_panel(status, report, theme, cx));
        page.into_any_element()
    }

    /// Tous les endroits qui s'écartent du pattern : fichier et ligne, son
    /// dossier en dessous (rien n'est coupé), et la règle concernée.
    fn findings_panel(
        &self,
        status: &PatternStatus,
        report: &AnalysisReport,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        let count = status.findings.len() as u32;
        let caption = if count == 0 {
            "À revoir".to_owned()
        } else {
            format!("À revoir · {}", count_label(count, "endroit", "endroits"))
        };
        let mut panel = panel(theme).child(section_caption(&caption, theme));
        if count == 0 {
            return panel.child(
                div()
                    .flex()
                    .items_center()
                    .gap(metrics::SPACE_2)
                    .text_size(metrics::TEXT_BASE)
                    .text_color(theme.success)
                    .child(Icon::new(IconName::Check, theme.success).size(metrics::ICON_SM))
                    .child("Rien à revoir, ce pattern est respecté dans tout le projet."),
            );
        }

        let rows = status
            .findings
            .iter()
            .take(SHOWN_FINDINGS)
            .enumerate()
            .map(|(row, finding)| {
                let issue = &report.issues[finding.issue];
                let (folder, file) = split_path(&issue.file);
                let location: SharedString = match issue.line {
                    Some(line) => format!("{file}:{line}").into(),
                    None => file.to_owned().into(),
                };
                let openable = issue.file.as_ref() != GIT_PSEUDO_FILE;
                let (path, line) = (issue.file.to_string(), issue.line);
                div()
                    .id(SharedString::from(format!("pattern-finding-{row}")))
                    .flex()
                    .items_center()
                    .gap(metrics::SPACE_4)
                    .mx(-metrics::SPACE_2)
                    .px(metrics::SPACE_2)
                    .py(metrics::SPACE_2)
                    .rounded(metrics::RADIUS)
                    .when(row > 0, |this| {
                        this.border_t_1().border_color(theme.border_soft)
                    })
                    .when(openable, |this| {
                        this.cursor_pointer()
                            .hover(|style| style.bg(theme.bg_hover))
                            .on_click(cx.listener(move |app, _, window, cx| {
                                app.open_in_ide(&path, line, window, cx)
                            }))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .font_family(fonts::MONO)
                                    .text_size(metrics::TEXT_SM)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(location),
                            )
                            .when(!folder.is_empty(), |this| {
                                this.child(
                                    div()
                                        .truncate()
                                        .font_family(fonts::MONO)
                                        .text_size(metrics::TEXT_XS)
                                        .text_color(theme.ink_soft)
                                        .child(folder.to_owned()),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(metrics::TEXT_SM)
                            .text_color(theme.ink_muted)
                            .child(rule_title(&issue.rule)),
                    )
            });
        panel = panel.child(div().flex().flex_col().children(rows));

        let hidden = status.findings.len().saturating_sub(SHOWN_FINDINGS) as u32;
        panel.when(hidden > 0, |this| {
            this.child(
                div()
                    .text_size(metrics::TEXT_SM)
                    .text_color(theme.ink_muted)
                    .child(format!(
                        "et {}",
                        count_label(hidden, "autre endroit", "autres endroits")
                    )),
            )
        })
    }
}

/// Bilan, puis les cartes, patterns à revoir en premier.
fn pattern_grid(
    statuses: &[PatternStatus],
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> AnyElement {
    let tracked = statuses.iter().filter(|s| s.pattern.is_some()).count();
    let respected = statuses
        .iter()
        .filter(|s| s.pattern.is_some() && s.is_respected())
        .count();

    let rows = statuses
        .chunks(GRID_COLUMNS)
        .enumerate()
        .map(|(row, chunk)| {
            let mut line = div().flex().gap(metrics::SPACE_4);
            for (col, status) in chunk.iter().enumerate() {
                line = line.child(pattern_card(row * GRID_COLUMNS + col, status, theme, cx));
            }
            // Ligne incomplète : des cases vides gardent la largeur des cartes.
            for _ in chunk.len()..GRID_COLUMNS {
                line = line.child(div().flex_1().flex_basis(relative(0.)).min_w_0());
            }
            line
        });

    div()
        .flex()
        .flex_col()
        .gap(metrics::SPACE_4)
        .child(
            div()
                .text_size(metrics::TEXT_SM)
                .text_color(theme.ink_muted)
                .child(format!(
                    "{respected} pattern{} sur {tracked} respecté{} dans ce projet. Cliquez sur un pattern pour le détail.",
                    if respected > 1 { "s" } else { "" },
                    if respected > 1 { "s" } else { "" },
                )),
        )
        .children(rows)
        .into_any_element()
}

/// Carte de la grille : icône, nom et état. Un clic ouvre la page du pattern.
fn pattern_card(
    ix: usize,
    status: &PatternStatus,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> AnyElement {
    let name = status.name();
    surface(theme)
        .id(SharedString::from(format!("pattern-card-{ix}")))
        .flex_1()
        .flex_basis(relative(0.))
        .min_w_0()
        .flex()
        .flex_col()
        .gap(metrics::SPACE_3)
        .p(metrics::SPACE_4)
        .cursor_pointer()
        .hover(|style| style.bg(theme.bg_hover))
        .on_click(cx.listener(move |app, _, _, cx| {
            app.open_pattern = Some(name);
            cx.notify();
        }))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(icon_tile(status, px(36.), px(20.), theme))
                .child(Icon::new(IconName::ChevronRight, theme.ink_soft).size(metrics::ICON_SM)),
        )
        .child(
            div()
                .text_size(metrics::TEXT_MD)
                .font_weight(FontWeight::SEMIBOLD)
                .child(name),
        )
        .child(state_badge(status, theme))
        .into_any_element()
}

/// Icône du pattern dans un carré teinté : vert s'il est respecté, ambre sinon.
fn icon_tile(status: &PatternStatus, size: Pixels, icon: Pixels, theme: Theme) -> Div {
    let (bg, tone) = tones(status, theme);
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(size)
        .rounded(metrics::RADIUS_LG)
        .bg(bg)
        .border_1()
        .border_color(theme.border_soft)
        .child(svg().path(status.icon()).size(icon).text_color(tone))
}

fn tones(status: &PatternStatus, theme: Theme) -> (Hsla, Hsla) {
    if status.is_respected() {
        (theme.success_bg, theme.success)
    } else {
        (theme.accent_soft, theme.warning)
    }
}

/// « ✓ Respecté » ou « N endroits à revoir ».
fn state_badge(status: &PatternStatus, theme: Theme) -> Div {
    if status.is_respected() {
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(metrics::SPACE_1)
            .text_size(metrics::TEXT_SM)
            .text_color(theme.success)
            .child(Icon::new(IconName::Check, theme.success).size(metrics::ICON_SM))
            .child("Respecté")
    } else {
        div()
            .flex_none()
            .text_size(metrics::TEXT_SM)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.warning)
            .child(count_label(
                status.findings.len() as u32,
                "endroit à revoir",
                "endroits à revoir",
            ))
    }
}

/// Bloc de la page d'un pattern.
fn panel(theme: Theme) -> Div {
    surface(theme)
        .w_full()
        .flex()
        .flex_col()
        .gap(metrics::SPACE_3)
        .p(metrics::SPACE_5)
}

/// Dossier et nom du fichier : `("src/billing/views", "invoice.js")`.
fn split_path(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some((folder, file)) => (folder, file),
        None => ("", path),
    }
}

/// Schéma « à éviter » ou « à faire » dans son bloc : une chaîne de blocs par ligne.
fn diagram_panel(title: &str, chains: Diagram, tone: Hsla, theme: Theme) -> Div {
    surface(theme)
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(metrics::SPACE_3)
        .p(metrics::SPACE_5)
        .border_t_2()
        .border_color(tone)
        .child(
            div()
                .text_size(metrics::TEXT_XS)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tone)
                .child(title.to_uppercase()),
        )
        .children(chains.iter().map(|chain| {
            let mut row = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(metrics::SPACE_1);
            for (ix, block) in chain.iter().enumerate() {
                if ix > 0 {
                    row = row.child(div().text_color(theme.ink_soft).child("→"));
                }
                row = row.child(
                    div()
                        .px(metrics::SPACE_2)
                        .py(metrics::SPACE_1)
                        .rounded(metrics::RADIUS)
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.bg_page)
                        .font_family(fonts::MONO)
                        .text_size(metrics::TEXT_SM)
                        .child(*block),
                );
            }
            row
        }))
}

#[cfg(test)]
mod tests {
    use gpui::AssetSource;

    use super::split_path;
    use crate::assets::Assets;
    use crate::model::patterns::{OTHERS_ICON, PATTERNS};

    #[test]
    fn paths_split_into_folder_and_file() {
        assert_eq!(
            split_path("src/billing/views/invoice.js"),
            ("src/billing/views", "invoice.js")
        );
        assert_eq!(split_path("Cargo.toml"), ("", "Cargo.toml"));
    }

    #[test]
    fn every_pattern_icon_is_embedded() {
        for icon in PATTERNS.iter().map(|p| p.icon).chain([OTHERS_ICON]) {
            assert!(
                Assets.load(icon).unwrap().is_some(),
                "{icon} absente de assets.rs"
            );
        }
    }
}

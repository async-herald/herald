//! Export du rapport d'analyse en Markdown ou en PDF.
//!
//! Le rapport est d'abord décrit en blocs (titre, sections, paragraphes,
//! éléments de liste), puis rendu dans l'un ou l'autre format : les deux
//! fichiers disent exactement la même chose. Rien n'est envoyé : le fichier est
//! écrit là où l'utilisateur l'a choisi.

use std::io::Cursor;

use printpdf::{
    Color, IndirectFontRef, Line, Mm, PdfDocument, PdfDocumentReference, PdfLayerReference, Point,
    Rgb,
};

use crate::model::{Advisory, AnalysisReport, rule_text};

/// Occurrences listées au plus par problème (le reste est résumé).
const MAX_OCCURRENCES: usize = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Pdf,
    Markdown,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Markdown => "md",
        }
    }
}

/// Élément du rapport, indépendant du format.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Block {
    Title(String),
    Meta(String),
    Heading(String),
    Subheading(String),
    Paragraph(String),
    Item(String),
}

/// Rapport du projet `project`, au format demandé.
pub fn render(
    format: Format,
    project: &str,
    report: &AnalysisReport,
    advisories: &[&Advisory],
) -> Result<Vec<u8>, String> {
    let blocks = blocks(project, report, advisories);
    match format {
        Format::Markdown => Ok(markdown(&blocks).into_bytes()),
        Format::Pdf => pdf(&blocks, &format!("Rapport Herald — {project}")),
    }
}

/// `herald-<projet>-<aaaa-mm-jj>.<ext>`.
pub fn file_name(project: &str, report: &AnalysisReport, format: Format) -> String {
    let slug: String = project
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    let date = crate::engine::cve::date_label(report.summary.analyzed_at_unix);
    // « 12/03/2024 » → « 2024-03-12 », trié correctement dans un dossier.
    let iso: Vec<&str> = date.split('/').rev().collect();
    format!("herald-{slug}-{}.{}", iso.join("-"), format.extension())
}

fn blocks(project: &str, report: &AnalysisReport, advisories: &[&Advisory]) -> Vec<Block> {
    let summary = &report.summary;
    let mut out = vec![
        Block::Title(format!("Rapport Herald — {project}")),
        Block::Meta(format!(
            "Analysé le {} · {} fichiers · {} langages · {} problèmes",
            crate::engine::cve::date_label(summary.analyzed_at_unix),
            summary.file_count,
            summary.language_count,
            summary.issue_count
        )),
        Block::Meta(format!(
            "Généré par Async Herald {} : analyse faite sur la machine, sans IA.",
            crate::VERSION
        )),
        Block::Heading("Note".into()),
    ];
    let delta = summary
        .delta()
        .map(|d| {
            format!(
                " ({}{d} depuis l'analyse précédente)",
                if d >= 0 { "+" } else { "" }
            )
        })
        .unwrap_or_default();
    out.push(Block::Paragraph(format!(
        "{} — {}/100{delta}",
        summary.grade(),
        summary.score
    )));

    out.push(Block::Heading("Par catégorie".into()));
    for category in &report.categories {
        let mut line = format!(
            "{} : {}/100 · {} problèmes",
            category.category.label(),
            category.score,
            category.issue_count
        );
        if !category.applicable {
            line.push_str(" · non comptée dans la note");
        }
        out.push(Block::Item(line));
    }

    out.push(Block::Heading("Problèmes".into()));
    let groups = report.group_indices(None);
    if groups.is_empty() {
        out.push(Block::Paragraph("Aucun problème relevé.".into()));
    }
    for gi in groups {
        let Some(group) = report.group(gi) else {
            continue;
        };
        let status = if group.review {
            "À revoir".to_owned()
        } else {
            group.severity.label().to_owned()
        };
        let resolved = if report.is_group_resolved(gi) {
            " · marqué comme corrigé"
        } else {
            ""
        };
        out.push(Block::Subheading(format!(
            "{} — {status} ×{}{resolved}",
            group.title,
            group.occurrences.len()
        )));
        match rule_text(&group.rule) {
            Some(text) => {
                out.push(Block::Paragraph(format!(
                    "Règle {} : {}",
                    group.rule, text.why
                )));
                out.push(Block::Paragraph(format!("Comment corriger : {}", text.fix)));
            }
            None => out.push(Block::Paragraph(format!("Règle {}.", group.rule))),
        }
        if group.review {
            out.push(Block::Paragraph(
                "Règle peu fiable sur le corpus de mesure : à vérifier, hors note.".into(),
            ));
        }
        for &ix in group.occurrences.iter().take(MAX_OCCURRENCES) {
            out.push(Block::Item(report.issues[ix].location().to_string()));
        }
        if group.occurrences.len() > MAX_OCCURRENCES {
            out.push(Block::Item(format!(
                "… et {} autres",
                group.occurrences.len() - MAX_OCCURRENCES
            )));
        }
    }

    out.push(Block::Heading("Dépendances vulnérables".into()));
    if advisories.is_empty() {
        out.push(Block::Paragraph(
            "Aucune vulnérabilité connue (base OSV) dans les dépendances.".into(),
        ));
    }
    for advisory in advisories {
        let fix = advisory.fixed_version.as_ref().map_or_else(
            || "pas encore de correctif".to_owned(),
            |v| format!("corrigé en {v}"),
        );
        let confirm = if advisory.confirmed {
            ""
        } else {
            " · version déclarée, à confirmer"
        };
        out.push(Block::Item(format!(
            "{} {} — {} ({}) : {} — {fix}{confirm}",
            advisory.package,
            advisory.installed_version,
            advisory.id,
            advisory.severity.label(),
            advisory.summary
        )));
    }
    out
}

fn markdown(blocks: &[Block]) -> String {
    let mut out = String::new();
    let mut in_list = false;
    for block in blocks {
        let is_item = matches!(block, Block::Item(_));
        if in_list && !is_item {
            out.push('\n');
        }
        in_list = is_item;
        match block {
            Block::Title(text) => out.push_str(&format!("# {text}\n\n")),
            Block::Meta(text) => out.push_str(&format!("_{text}_\n\n")),
            Block::Heading(text) => out.push_str(&format!("## {text}\n\n")),
            Block::Subheading(text) => out.push_str(&format!("### {text}\n\n")),
            Block::Paragraph(text) => out.push_str(&format!("{text}\n\n")),
            Block::Item(text) => out.push_str(&format!("- {text}\n")),
        }
    }
    out.trim_end().to_owned() + "\n"
}

// ── PDF ────────────────────────────────────────────────────────────────────

const PAGE_W: f32 = 210.;
const PAGE_H: f32 = 297.;
const MARGIN: f32 = 20.;
const BOTTOM: f32 = 18.;
/// Points typographiques → millimètres.
const PT: f32 = 0.3528;

struct Style {
    size: f32,
    bold: bool,
    gray: f32,
    before: f32,
    after: f32,
    indent: f32,
}

fn style(block: &Block) -> Style {
    let s = |size, bold, gray, before, after, indent| Style {
        size,
        bold,
        gray,
        before,
        after,
        indent,
    };
    match block {
        Block::Title(_) => s(18., true, 0.1, 0., 3., 0.),
        Block::Meta(_) => s(9., false, 0.45, 0., 1.5, 0.),
        Block::Heading(_) => s(13., true, 0.1, 6., 2.5, 0.),
        Block::Subheading(_) => s(10.5, true, 0.15, 4., 1.5, 0.),
        Block::Paragraph(_) => s(9.5, false, 0.2, 0., 1.5, 0.),
        Block::Item(_) => s(9., false, 0.25, 0., 0.8, 4.),
    }
}

fn text_of(block: &Block) -> &str {
    match block {
        Block::Title(t)
        | Block::Meta(t)
        | Block::Heading(t)
        | Block::Subheading(t)
        | Block::Paragraph(t)
        | Block::Item(t) => t,
    }
}

/// Découpe `text` en lignes d'au plus `width` mm (largeur moyenne des
/// caractères d'Inter estimée ; un mot trop long est coupé).
fn wrap(text: &str, size: f32, bold: bool, width: f32) -> Vec<String> {
    let char_mm = size * PT * if bold { 0.58 } else { 0.54 };
    let max = ((width / char_mm) as usize).max(10);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut word = word.to_owned();
        while word.chars().count() > max {
            let head: String = word.chars().take(max).collect();
            word = word.chars().skip(max).collect();
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            lines.push(head);
        }
        let needed = line.chars().count() + usize::from(!line.is_empty()) + word.chars().count();
        if needed > max && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

struct Writer {
    doc: PdfDocumentReference,
    layer: PdfLayerReference,
    regular: IndirectFontRef,
    bold: IndirectFontRef,
    y: f32,
    page: usize,
    title: String,
}

impl Writer {
    fn new_page(&mut self) {
        let (page, layer) = self.doc.add_page(Mm(PAGE_W), Mm(PAGE_H), "Page");
        self.layer = self.doc.get_page(page).get_layer(layer);
        self.page += 1;
        self.y = PAGE_H - MARGIN;
        self.footer();
    }

    fn footer(&self) {
        self.layer.set_fill_color(gray(0.55));
        self.layer.use_text(
            format!("{} · page {}", self.title, self.page),
            8.,
            Mm(MARGIN),
            Mm(10.),
            &self.regular,
        );
    }

    fn rule(&self, y: f32) {
        self.layer.set_outline_color(gray(0.85));
        self.layer.set_outline_thickness(0.5);
        self.layer.add_line(Line {
            points: vec![
                (Point::new(Mm(MARGIN), Mm(y)), false),
                (Point::new(Mm(PAGE_W - MARGIN), Mm(y)), false),
            ],
            is_closed: false,
        });
    }

    fn block(&mut self, block: &Block) {
        let style = style(block);
        let line_h = style.size * PT * 1.45;
        let width = PAGE_W - 2. * MARGIN - style.indent;
        let mut lines = wrap(text_of(block), style.size, style.bold, width);
        if matches!(block, Block::Item(_)) {
            if let Some(first) = lines.first_mut() {
                *first = format!("•  {first}");
            }
        }
        self.y -= style.before;
        // Un titre de section n'est jamais seul en bas de page.
        let keep = if matches!(block, Block::Heading(_) | Block::Subheading(_)) {
            3.
        } else {
            1.
        };
        if self.y - line_h * keep < BOTTOM {
            self.new_page();
        }
        let font = if style.bold {
            self.bold.clone()
        } else {
            self.regular.clone()
        };
        for line in lines {
            if self.y - line_h < BOTTOM {
                self.new_page();
            }
            self.y -= line_h;
            self.layer.set_fill_color(gray(style.gray));
            self.layer.use_text(
                line,
                style.size,
                Mm(MARGIN + style.indent),
                Mm(self.y),
                &font,
            );
        }
        if matches!(block, Block::Heading(_)) {
            self.rule(self.y - 1.2);
            self.y -= 1.2;
        }
        self.y -= style.after;
    }
}

fn gray(level: f32) -> Color {
    Color::Rgb(Rgb::new(level, level, level, None))
}

fn pdf(blocks: &[Block], title: &str) -> Result<Vec<u8>, String> {
    let (doc, page, layer) = PdfDocument::new(title, Mm(PAGE_W), Mm(PAGE_H), "Page");
    let font = |path: &str| {
        let bytes = crate::assets::font(path).ok_or_else(|| format!("police {path} absente"))?;
        doc.add_external_font(Cursor::new(bytes))
            .map_err(|err| format!("police {path} : {err}"))
    };
    let regular = font("fonts/inter-400.ttf")?;
    let bold = font("fonts/inter-600.ttf")?;
    let layer = doc.get_page(page).get_layer(layer);
    let mut writer = Writer {
        doc,
        layer,
        regular,
        bold,
        y: PAGE_H - MARGIN,
        page: 1,
        title: title.to_owned(),
    };
    writer.footer();
    for block in blocks {
        writer.block(block);
    }
    writer
        .doc
        .save_to_bytes()
        .map_err(|err| format!("écriture du PDF : {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Écrit un exemple de rapport PDF pour le relire à l'œil :
    /// `HERALD_EXPORT_SAMPLE=/tmp/r.pdf cargo test -- --ignored sample_pdf`.
    #[test]
    #[ignore]
    fn sample_pdf() {
        let Some(path) = std::env::var_os("HERALD_EXPORT_SAMPLE") else {
            return;
        };
        let mut blocks = vec![
            Block::Title("Rapport Herald — demo-repo".into()),
            Block::Meta("Analysé le 11/11/2026 · 42 fichiers · 4 langages · 205 problèmes".into()),
            Block::Meta(
                "Généré par Async Herald 0.1.2 : analyse faite sur la machine, sans IA.".into(),
            ),
            Block::Heading("Note".into()),
            Block::Paragraph("F — 5/100 (+2 depuis l'analyse précédente)".into()),
            Block::Heading("Par catégorie".into()),
            Block::Item("Sécurité : 12/100 · 31 problèmes".into()),
            Block::Item("Qualité : 40/100 · 150 problèmes".into()),
            Block::Item("Tests : 100/100 · 0 problèmes · non comptée dans la note".into()),
            Block::Heading("Problèmes".into()),
        ];
        for (title, rule) in [
            ("Secret en dur — Critique ×3", "hardcoded_secret"),
            ("Variable inutilisée — Attention ×24", "no_unused_vars"),
        ] {
            blocks.push(Block::Subheading(title.into()));
            match rule_text(rule) {
                Some(text) => {
                    blocks.push(Block::Paragraph(format!("Règle {rule} : {}", text.why)));
                    blocks.push(Block::Paragraph(format!("Comment corriger : {}", text.fix)));
                }
                None => blocks.push(Block::Paragraph(format!("Règle {rule}."))),
            }
            for line in 1..6 {
                blocks.push(Block::Item(format!(
                    "src/legacy/kitchen-sink.js:{}",
                    line * 12
                )));
            }
        }
        blocks.push(Block::Heading("Dépendances vulnérables".into()));
        blocks.push(Block::Item(
            "lodash 4.17.15 — CVE-2020-8203 (Critique) : Prototype pollution in lodash — corrigé en 4.17.19".into(),
        ));
        std::fs::write(path, pdf(&blocks, "Rapport Herald — demo-repo").unwrap()).unwrap();
    }

    #[test]
    fn wrapping_keeps_words_and_width() {
        let text = "Une phrase assez longue pour devoir passer sur plusieurs lignes du rapport PDF";
        let lines = wrap(text, 10., false, 40.);
        assert!(lines.len() > 1);
        assert_eq!(lines.join(" "), text);
        let max = (40. / (10. * PT * 0.54)) as usize;
        assert!(lines.iter().all(|l| l.chars().count() <= max));
        // Un chemin plus long que la ligne est coupé, pas perdu.
        let long = "a".repeat(200);
        assert_eq!(wrap(&long, 10., false, 40.).concat(), long);
    }

    #[test]
    fn markdown_and_pdf_render_the_same_blocks() {
        let blocks = vec![
            Block::Title("Rapport Herald — démo".into()),
            Block::Meta("Analysé le 11/11/2026".into()),
            Block::Heading("Problèmes".into()),
            Block::Subheading("Secret en dur — Critique ×2".into()),
            Block::Paragraph("Comment corriger : déplacez-le.".into()),
            Block::Item("src/config.js:12".into()),
            Block::Item("src/db.js:3".into()),
            Block::Heading("Dépendances vulnérables".into()),
        ];
        let md = markdown(&blocks);
        assert!(md.starts_with("# Rapport Herald — démo\n"));
        assert!(md.contains("### Secret en dur — Critique ×2\n"));
        assert!(md.contains("- src/config.js:12\n- src/db.js:3\n\n## Dépendances"));

        let bytes = pdf(&blocks, "Rapport Herald — démo").unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        // Beaucoup de lignes : plusieurs pages, sans erreur.
        let many: Vec<Block> = (0..400)
            .map(|i| Block::Item(format!("src/f{i}.rs:{i}")))
            .collect();
        let long = pdf(&many, "long").unwrap();
        assert!(long.len() > bytes.len());
    }
}

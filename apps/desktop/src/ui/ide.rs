//! Vue « Code » de l'écran d'analyse : explorateur du projet, onglets, éditeur
//! avec les problèmes dans la marge, barre d'état et liste des problèmes du fichier.
//!
//! Les fichiers sont lus et réécrits sur le disque ; rien n'est envoyé.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{
    AnyElement, Entity, Focusable, FontWeight, PromptButton, PromptLevel, ScrollStrategy,
    SharedString, Subscription, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};

use super::app::{HeraldApp, WorkspaceView};
use super::components::{
    Button, ButtonVariant, CodeArea, CodeAreaEvent, Diagnostic, Icon, IconName, SeverityLabel,
};
use super::home::{count_label, surface};
use crate::model::{AnalysisReport, Severity};
use crate::theme::{Theme, fonts, metrics};

const EXPLORER_WIDTH: gpui::Pixels = px(260.);
const PROBLEMS_HEIGHT: gpui::Pixels = px(168.);
/// Retrait par niveau de l'explorateur.
const TREE_INDENT: gpui::Pixels = px(16.);
/// Marge gauche du premier niveau.
const TREE_LEFT: gpui::Pixels = px(8.);
const CHEVRON_WIDTH: gpui::Pixels = px(16.);
const CHEVRON_ICON: gpui::Pixels = px(12.);
/// Guide d'indentation vertical, sous le chevron du dossier parent.
const GUIDE_WIDTH: gpui::Pixels = px(1.);
const SEVERITY_WIDTH: gpui::Pixels = px(88.);
/// Point « modifié » d'un onglet.
const DIRTY_DOT: gpui::Pixels = px(8.);
const LINE_LABEL_WIDTH: gpui::Pixels = px(56.);
/// Place prise au-dessus de l'IDE (titre, marges de l'écran).
const IDE_CHROME: gpui::Pixels = px(120.);
const IDE_MIN_HEIGHT: gpui::Pixels = px(480.);
/// Au-delà, le fichier n'est pas ouvert dans l'éditeur.
const MAX_OPEN_BYTES: u64 = 2 * 1024 * 1024;
/// Problèmes du dépôt entier, sans fichier à ouvrir.
const GIT_PSEUDO_FILE: &str = "Historique Git";

/// État de l'IDE d'un projet (gardé quand on change de projet).
pub(super) struct Ide {
    tree: Vec<TreeNode>,
    expanded: HashSet<String>,
    /// Explorateur limité aux fichiers qui ont des problèmes.
    only_problems: bool,
    /// Change avec l'arbre, les dossiers ouverts ou le filtre : invalide `rows`.
    generation: u64,
    /// Lignes visibles de l'explorateur, recalculées seulement quand l'arbre,
    /// les dossiers ouverts, le filtre ou les corrections changent (pas à
    /// chaque frappe dans l'éditeur, qui redessine toute la fenêtre).
    rows: RefCell<Option<(u64, Rc<Vec<ExplorerRow>>)>>,
    scroll: UniformListScrollHandle,
    /// Faire défiler l'explorateur jusqu'au fichier actif au prochain rendu.
    reveal: Cell<bool>,
    tabs: Vec<OpenFile>,
    active: Option<usize>,
    notice: Option<Notice>,
}

/// Ligne visible de l'explorateur.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ExplorerRow {
    /// Chemin du nœud (le plus profond pour une chaîne de dossiers compactée).
    path: String,
    /// Nom affiché : `src/main/java` pour des dossiers à enfant unique.
    label: String,
    depth: usize,
    is_dir: bool,
    open: bool,
    /// Problèmes ouverts et gravité la plus forte.
    count: Option<(usize, Severity)>,
}

/// Lignes visibles, dossiers à enfant unique regroupés sur une ligne.
fn explorer_rows(
    nodes: &[TreeNode],
    expanded: &HashSet<String>,
    counts: &HashMap<String, (usize, Severity)>,
    only_problems: bool,
) -> Vec<ExplorerRow> {
    fn push(
        nodes: &[TreeNode],
        depth: usize,
        expanded: &HashSet<String>,
        counts: &HashMap<String, (usize, Severity)>,
        only_problems: bool,
        out: &mut Vec<ExplorerRow>,
    ) {
        for node in nodes {
            let mut last = node;
            let mut label = node.name.clone();
            while last.is_dir && last.children.len() == 1 && last.children[0].is_dir {
                last = &last.children[0];
                label.push('/');
                label.push_str(&last.name);
            }
            let count = counts.get(&last.path).copied();
            if only_problems && count.is_none() {
                continue;
            }
            let open = last.is_dir && expanded.contains(&last.path);
            out.push(ExplorerRow {
                path: last.path.clone(),
                label,
                depth,
                is_dir: last.is_dir,
                open,
                count,
            });
            if open {
                push(
                    &last.children,
                    depth + 1,
                    expanded,
                    counts,
                    only_problems,
                    out,
                );
            }
        }
    }
    let mut out = Vec::new();
    push(nodes, 0, expanded, counts, only_problems, &mut out);
    out
}

struct Notice {
    text: SharedString,
    error: bool,
}

struct OpenFile {
    relative: SharedString,
    absolute: PathBuf,
    editor: Entity<CodeArea>,
    /// Contenu sur le disque à l'ouverture ou au dernier enregistrement.
    disk: String,
    crlf: bool,
    _subscription: Subscription,
}

#[derive(Debug, PartialEq, Eq)]
struct TreeNode {
    name: String,
    path: String,
    children: Vec<TreeNode>,
    is_dir: bool,
}

/// Arbre trié (dossiers d'abord) à partir de chemins relatifs.
fn build_tree(paths: &[String]) -> Vec<TreeNode> {
    #[derive(Default)]
    struct Dir {
        dirs: BTreeMap<String, Dir>,
        files: Vec<String>,
    }
    let mut root = Dir::default();
    for path in paths {
        let mut parts: Vec<&str> = path.split('/').collect();
        let Some(file) = parts.pop() else { continue };
        let mut dir = &mut root;
        for part in parts {
            dir = dir.dirs.entry(part.to_owned()).or_default();
        }
        dir.files.push(file.to_owned());
    }
    fn convert(dir: Dir, prefix: &str) -> Vec<TreeNode> {
        let join = |name: &str| {
            if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}/{name}")
            }
        };
        let mut dirs: Vec<(String, Dir)> = dir.dirs.into_iter().collect();
        dirs.sort_by_key(|(name, _)| name.to_lowercase());
        let mut files = dir.files;
        files.sort_by_key(|name| name.to_lowercase());
        dirs.into_iter()
            .map(|(name, sub)| {
                let path = join(&name);
                TreeNode {
                    children: convert(sub, &path),
                    name,
                    path,
                    is_dir: true,
                }
            })
            .chain(files.into_iter().map(|name| TreeNode {
                path: join(&name),
                name,
                children: Vec::new(),
                is_dir: false,
            }))
            .collect()
    }
    convert(root, "")
}

/// Problèmes par fichier, avec la gravité
/// la plus forte ; les dossiers cumulent ceux de leurs fichiers.
fn issue_counts(report: Option<&AnalysisReport>) -> HashMap<String, (usize, Severity)> {
    let mut counts: HashMap<String, (usize, Severity)> = HashMap::new();
    let Some(report) = report else {
        return counts;
    };
    for (ix, issue) in report.issues.iter().enumerate() {
        if report.is_resolved(ix) || issue.file == GIT_PSEUDO_FILE {
            continue;
        }
        let path = issue.file.as_ref();
        let mut prefixes: Vec<&str> = path.match_indices('/').map(|(i, _)| &path[..i]).collect();
        prefixes.push(path);
        for prefix in prefixes {
            let entry = counts
                .entry(prefix.to_owned())
                .or_insert((0, issue.severity));
            entry.0 += 1;
            entry.1 = entry.1.min(issue.severity);
        }
    }
    counts
}

/// Marqueurs de la marge pour un fichier.
fn diagnostics_for(report: Option<&AnalysisReport>, relative: &str) -> Vec<Diagnostic> {
    let Some(report) = report else {
        return Vec::new();
    };
    report
        .issues
        .iter()
        .enumerate()
        .filter(|(ix, issue)| issue.file == relative && !report.is_resolved(*ix))
        .filter_map(|(_, issue)| {
            Some(Diagnostic {
                row: issue.line?.saturating_sub(1) as usize,
                severity: issue.severity,
                message: match &issue.explanation {
                    Some(text) => format!("{} — {text}", issue.title).into(),
                    None => issue.title.clone(),
                },
            })
        })
        .collect()
}

fn severity_color(severity: Severity, theme: Theme) -> gpui::Hsla {
    match severity {
        Severity::Critical => theme.danger,
        Severity::Warning => theme.warning,
        Severity::Info => theme.ink_soft,
    }
}

impl Ide {
    fn new(root: &Path, report: Option<&AnalysisReport>) -> Self {
        let tree = build_tree(&crate::engine::collect::project_files(root));
        // Dossiers ouverts au départ : ceux qui mènent à un problème.
        let expanded = issue_counts(report)
            .into_keys()
            .filter(|path| tree_contains_dir(&tree, path))
            .collect();
        Self {
            tree,
            expanded,
            only_problems: false,
            generation: 0,
            rows: RefCell::new(None),
            scroll: UniformListScrollHandle::new(),
            reveal: Cell::new(false),
            tabs: Vec::new(),
            active: None,
            notice: None,
        }
    }

    fn active_file(&self) -> Option<&OpenFile> {
        self.tabs.get(self.active?)
    }

    /// L'arbre, les dossiers ouverts ou le filtre ont changé.
    fn invalidate(&mut self) {
        self.generation += 1;
    }

    /// Lignes visibles, depuis le cache tant que rien n'a changé.
    fn rows(&self, report: Option<&AnalysisReport>) -> Rc<Vec<ExplorerRow>> {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.generation.hash(&mut hasher);
        if let Some(report) = report {
            report.summary.analyzed_at_unix.hash(&mut hasher);
            report.resolved_indices().hash(&mut hasher);
        }
        let key = hasher.finish();
        if let Some((cached, rows)) = &*self.rows.borrow() {
            if *cached == key {
                return rows.clone();
            }
        }
        let rows = Rc::new(explorer_rows(
            &self.tree,
            &self.expanded,
            &issue_counts(report),
            self.only_problems,
        ));
        *self.rows.borrow_mut() = Some((key, rows.clone()));
        rows
    }
}

fn tree_contains_dir(nodes: &[TreeNode], path: &str) -> bool {
    nodes.iter().any(|node| {
        node.is_dir
            && (node.path == path
                || (path.starts_with(&format!("{}/", node.path))
                    && tree_contains_dir(&node.children, path)))
    })
}

impl HeraldApp {
    fn ide_mut(&mut self) -> Option<&mut Ide> {
        let path = self.project()?.path.clone();
        if !self.ides.contains_key(&path) {
            let ide = Ide::new(&path, self.report());
            self.ides.insert(path.clone(), ide);
        }
        self.ides.get_mut(&path)
    }

    fn ide(&self) -> Option<&Ide> {
        self.ides.get(&self.project()?.path)
    }

    pub(super) fn set_workspace_view(
        &mut self,
        view: WorkspaceView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace_view = view;
        if view == WorkspaceView::Code {
            let first_issue = self.ide_mut().is_some_and(|ide| ide.tabs.is_empty());
            if first_issue {
                // Premier passage : ouvre le fichier du problème le plus grave.
                let target = self.selected_issue_index().and_then(|ix| {
                    let issue = self.report()?.issues.get(ix)?;
                    (issue.file != GIT_PSEUDO_FILE).then(|| (issue.file.to_string(), issue.line))
                });
                if let Some((file, line)) = target {
                    self.open_in_ide(&file, line, window, cx);
                }
            } else if let Some(file) = self.ide().and_then(Ide::active_file) {
                let handle = file.editor.read(cx).focus_handle(cx);
                window.focus(&handle);
            }
        }
        cx.notify();
    }

    /// Ouvre `relative` (ou passe à son onglet) et place le curseur sur `line`.
    pub(super) fn open_in_ide(
        &mut self,
        relative: &str,
        line: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace_view = WorkspaceView::Code;
        let Some(root) = self.project().map(|p| p.path.clone()) else {
            return;
        };
        let diagnostics = diagnostics_for(self.report(), relative);
        let existing = self
            .ide_mut()
            .and_then(|ide| ide.tabs.iter().position(|tab| tab.relative == relative));

        let ix = match existing {
            Some(ix) => ix,
            None => {
                let absolute = root.join(relative);
                let raw = match read_text(&absolute) {
                    Ok(raw) => raw,
                    Err(message) => {
                        if let Some(ide) = self.ide_mut() {
                            ide.notice = Some(Notice {
                                text: message.into(),
                                error: true,
                            });
                        }
                        cx.notify();
                        return;
                    }
                };
                let editor = cx.new(|cx| {
                    let mut area = CodeArea::new(&raw, relative, cx);
                    area.set_diagnostics(diagnostics, cx);
                    area
                });
                let subscription = cx.subscribe(&editor, {
                    let relative = SharedString::from(relative.to_owned());
                    // L'éditeur se redessine seul. L'app ne se redessine que si ce
                    // qu'elle affiche change : l'état « modifié » (onglet, bouton
                    // Enregistrer) ou le nombre de lignes (numéros du panneau Problèmes).
                    let seen = std::cell::Cell::new((false, 0usize));
                    move |app, editor, event, cx| match event {
                        CodeAreaEvent::Save => app.save_ide_file(&relative, cx),
                        CodeAreaEvent::Edited => {
                            let area = editor.read(cx);
                            let now = (area.is_dirty(), area.line_count());
                            if seen.replace(now) != now {
                                cx.notify();
                            }
                        }
                        CodeAreaEvent::CursorMoved => {}
                    }
                });
                let Some(ide) = self.ide_mut() else { return };
                ide.tabs.push(OpenFile {
                    relative: relative.to_owned().into(),
                    absolute,
                    editor,
                    crlf: raw.contains("\r\n"),
                    disk: raw,
                    _subscription: subscription,
                });
                ide.tabs.len() - 1
            }
        };

        let Some(ide) = self.ide_mut() else { return };
        ide.active = Some(ix);
        ide.notice = None;
        // Ouvre les dossiers qui mènent au fichier.
        let mut prefix = String::new();
        for part in relative
            .split('/')
            .collect::<Vec<_>>()
            .iter()
            .rev()
            .skip(1)
            .rev()
        {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            ide.expanded.insert(prefix.clone());
        }
        ide.invalidate();
        ide.reveal.set(true);
        let editor = ide.tabs[ix].editor.clone();
        if let Some(line) = line {
            editor.update(cx, |area, cx| area.go_to_line(line as usize, cx));
        }
        let handle = editor.read(cx).focus_handle(cx);
        window.focus(&handle);
        cx.notify();
    }

    fn toggle_dir(&mut self, path: String, cx: &mut Context<Self>) {
        if let Some(ide) = self.ide_mut() {
            if !ide.expanded.remove(&path) {
                ide.expanded.insert(path);
            }
            ide.invalidate();
        }
        cx.notify();
    }

    /// « Problèmes » : seuls les fichiers qui ont des problèmes, dossiers ouverts.
    fn set_only_problems(&mut self, only: bool, cx: &mut Context<Self>) {
        let counts = issue_counts(self.report());
        let Some(ide) = self.ide_mut() else { return };
        if only {
            let dirs: Vec<String> = counts
                .into_keys()
                .filter(|path| tree_contains_dir(&ide.tree, path))
                .collect();
            ide.expanded.extend(dirs);
        }
        ide.only_problems = only;
        ide.invalidate();
        ide.reveal.set(true);
        cx.notify();
    }

    fn collapse_all(&mut self, cx: &mut Context<Self>) {
        if let Some(ide) = self.ide_mut() {
            ide.expanded.clear();
            ide.invalidate();
        }
        cx.notify();
    }

    fn activate_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ide) = self.ide_mut() else { return };
        if ix < ide.tabs.len() {
            ide.active = Some(ix);
            ide.notice = None;
            ide.reveal.set(true);
            let editor = ide.tabs[ix].editor.clone();
            let handle = editor.read(cx).focus_handle(cx);
            window.focus(&handle);
        }
        cx.notify();
    }

    fn request_close_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.ide().and_then(|ide| ide.tabs.get(ix)) else {
            return;
        };
        if !tab.editor.read(cx).is_dirty() {
            return self.close_tab(&tab.relative.clone(), cx);
        }
        let relative = tab.relative.clone();
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Fermer « {relative} » sans enregistrer ?"),
            Some("Les modifications non enregistrées seront perdues."),
            &[
                PromptButton::ok("Fermer sans enregistrer"),
                PromptButton::cancel("Annuler"),
            ],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await == Ok(0) {
                this.update(cx, |app, cx| app.close_tab(&relative, cx)).ok();
            }
        })
        .detach();
    }

    fn close_tab(&mut self, relative: &str, cx: &mut Context<Self>) {
        let Some(ide) = self.ide_mut() else { return };
        let Some(ix) = ide.tabs.iter().position(|tab| tab.relative == relative) else {
            return;
        };
        ide.tabs.remove(ix);
        ide.active = match ide.active {
            _ if ide.tabs.is_empty() => None,
            Some(active) if active > ix => Some(active - 1),
            Some(active) if active == ix => Some(ix.min(ide.tabs.len() - 1)),
            other => other,
        };
        cx.notify();
    }

    /// Enregistre l'onglet sur le disque, s'il n'a pas changé entre-temps.
    fn save_ide_file(&mut self, relative: &str, cx: &mut Context<Self>) {
        let Some(ide) = self.ide_mut() else { return };
        let Some(tab) = ide.tabs.iter_mut().find(|tab| tab.relative == relative) else {
            return;
        };
        let text = tab.editor.read(cx).text().to_owned();
        let content = if tab.crlf {
            text.replace('\n', "\r\n")
        } else {
            text.clone()
        };
        let result = match std::fs::read_to_string(&tab.absolute) {
            Ok(current) if current != tab.disk => Err(
                "Le fichier a changé sur le disque depuis son ouverture. Fermez l'onglet puis \
                 rouvrez-le."
                    .to_owned(),
            ),
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("Lecture impossible : {err}"))
            }
            _ => std::fs::write(&tab.absolute, &content)
                .map_err(|err| format!("Écriture impossible : {err}")),
        };
        let notice = match result {
            Ok(()) => {
                tab.disk = content;
                tab.editor.update(cx, |area, cx| area.mark_saved(cx));
                Notice {
                    text: format!(
                        "{relative} enregistré. Relancez l'analyse pour mettre la note à jour."
                    )
                    .into(),
                    error: false,
                }
            }
            Err(message) => Notice {
                text: message.into(),
                error: true,
            },
        };
        let saved = !notice.error;
        ide.notice = Some(notice);

        // L'extrait de la fiche reprend le nouveau contenu.
        if saved {
            if let Some(source) = self
                .projects
                .get_mut(self.selected_project)
                .and_then(|project| project.report.as_mut())
                .and_then(|report| report.sources.iter_mut().find(|s| s.path == relative))
            {
                source.lines = text
                    .lines()
                    .map(|line| SharedString::from(line.to_owned()))
                    .collect();
            }
        }
        cx.notify();
    }

    fn save_active_file(&mut self, cx: &mut Context<Self>) {
        if let Some(relative) = self
            .ide()
            .and_then(Ide::active_file)
            .map(|tab| tab.relative.clone())
        {
            self.save_ide_file(&relative, cx);
        }
    }

    /// Après une analyse : arbre relu et marqueurs mis à jour (onglets non modifiés).
    pub(super) fn refresh_ide(&mut self, root: &Path, cx: &mut Context<Self>) {
        let report = self
            .projects
            .iter()
            .find(|project| project.path == root)
            .and_then(|project| project.report.as_ref());
        let Some(ide) = self.ides.get_mut(root) else {
            return;
        };
        ide.tree = build_tree(&crate::engine::collect::project_files(root));
        ide.invalidate();
        for tab in &ide.tabs {
            let diagnostics = diagnostics_for(report, &tab.relative);
            tab.editor.update(cx, |area, cx| {
                if !area.is_dirty() {
                    area.set_diagnostics(diagnostics, cx);
                }
            });
        }
        ide.notice = None;
    }

    pub(super) fn forget_ide(&mut self, root: &Path) {
        self.ides.remove(root);
    }

    /// Crée l'état de l'IDE du projet courant avant son premier affichage.
    pub(super) fn ensure_ide(&mut self) {
        self.ide_mut();
    }

    pub(super) fn render_ide(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        if self.ide().is_none() {
            return div().into_any_element();
        }
        let height = (self.viewport_height - IDE_CHROME).max(IDE_MIN_HEIGHT);
        surface(theme)
            .flex_none()
            .h(height)
            .flex()
            .child(self.render_explorer(theme, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(self.render_tabs(theme, cx))
                    .child(self.render_editor(theme))
                    .child(self.render_status_bar(theme, cx))
                    .child(self.render_problems(theme, cx)),
            )
            .into_any_element()
    }

    fn render_explorer(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let Some(ide) = self.ide() else {
            return div().into_any_element();
        };
        let rows = ide.rows(self.report());
        // Fichier actif mis en vue après une ouverture ou un changement d'onglet.
        if ide.reveal.replace(false) {
            if let Some(active) = ide.active_file() {
                if let Some(ix) = rows
                    .iter()
                    .position(|row| row.path == active.relative.as_ref())
                {
                    ide.scroll.scroll_to_item(ix, ScrollStrategy::Center);
                }
            }
        }
        let empty = match (ide.tree.is_empty(), ide.only_problems) {
            (true, _) => Some("Dossier vide ou introuvable."),
            (false, true) if rows.is_empty() => Some("Aucun problème ouvert dans ce projet."),
            _ => None,
        };

        div()
            .w(EXPLORER_WIDTH)
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(theme.border_soft)
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(metrics::SPACE_2)
                    .pl(metrics::SPACE_3)
                    .pr(metrics::SPACE_1)
                    .h(metrics::CONTROL_H + metrics::SPACE_2)
                    .border_b_1()
                    .border_color(theme.border_soft)
                    .child(
                        div()
                            .flex_1()
                            .text_size(metrics::TEXT_XS)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.ink_soft)
                            .child("Explorateur"),
                    )
                    .child(explorer_filter(ide.only_problems, theme, cx))
                    .child(
                        Button::new("explorer-collapse", "Replier", theme)
                            .variant(ButtonVariant::Ghost)
                            .on_click(cx.listener(|app, _, _, cx| app.collapse_all(cx))),
                    ),
            )
            .children(empty.map(|text| {
                div()
                    .px(metrics::SPACE_3)
                    .py(metrics::SPACE_2)
                    .text_size(metrics::TEXT_SM)
                    .text_color(theme.ink_soft)
                    .child(text)
            }))
            .child(
                uniform_list(
                    "explorer",
                    rows.len(),
                    cx.processor(|app, range: Range<usize>, _, cx| app.explorer_items(range, cx)),
                )
                .track_scroll(ide.scroll.clone())
                .flex_1()
                .min_h_0()
                .py(metrics::SPACE_1),
            )
            .into_any_element()
    }

    /// Lignes `range` de l'explorateur (seules les lignes visibles sont construites).
    fn explorer_items(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = Theme::light();
        let Some(ide) = self.ide() else {
            return Vec::new();
        };
        let rows = ide.rows(self.report());
        let active = ide.active_file().map(|tab| tab.relative.clone());
        range
            .filter_map(|ix| {
                let row = rows.get(ix)?;
                let active = active.as_ref().is_some_and(|a| a.as_ref() == row.path);
                Some(explorer_row(ix, row, active, theme, cx))
            })
            .collect()
    }

    fn render_tabs(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let Some(ide) = self.ide() else {
            return div().into_any_element();
        };
        let tabs = ide
            .tabs
            .iter()
            .enumerate()
            .map(|(ix, tab)| {
                let active = ide.active == Some(ix);
                let dirty = tab.editor.read(cx).is_dirty();
                let name = tab
                    .relative
                    .rsplit('/')
                    .next()
                    .unwrap_or(&tab.relative)
                    .to_owned();
                let group = SharedString::from(format!("tab-{ix}"));
                div()
                    .id(("ide-tab", ix))
                    .group(group.clone())
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(metrics::SPACE_2)
                    .h_full()
                    .pl(metrics::SPACE_3)
                    .pr(metrics::SPACE_2)
                    .border_r_1()
                    .border_color(theme.border_soft)
                    .cursor_pointer()
                    .text_size(metrics::TEXT_SM)
                    .when(active, |this| {
                        this.bg(theme.code_bg)
                            .font_weight(FontWeight::MEDIUM)
                            .border_b_2()
                            .border_color(theme.accent)
                    })
                    .when(!active, |this| {
                        this.text_color(theme.ink_muted)
                            .hover(|style| style.bg(theme.bg_hover))
                    })
                    .on_click(
                        cx.listener(move |app, _, window, cx| app.activate_tab(ix, window, cx)),
                    )
                    .child(div().font_family(fonts::MONO).child(name))
                    .child(
                        div()
                            .id(("ide-tab-close", ix))
                            .flex_none()
                            .w(metrics::ICON)
                            .flex()
                            .justify_center()
                            .rounded(metrics::RADIUS)
                            .hover(|style| style.bg(theme.border_soft))
                            .on_click(cx.listener(move |app, _, window, cx| {
                                cx.stop_propagation();
                                app.request_close_tab(ix, window, cx);
                            }))
                            .child(if dirty {
                                // Point « modifié », remplacé par la croix au survol.
                                div()
                                    .size(DIRTY_DOT)
                                    .rounded_full()
                                    .bg(theme.ink_muted)
                                    .group_hover(group.clone(), |style| style.opacity(0.))
                                    .into_any_element()
                            } else {
                                Icon::new(IconName::Close, theme.ink_soft)
                                    .size(metrics::ICON_SM)
                                    .into_any_element()
                            }),
                    )
            })
            .collect::<Vec<_>>();

        div()
            .id("ide-tabs")
            .flex_none()
            .h(metrics::CONTROL_H + metrics::SPACE_2)
            .flex()
            .overflow_x_scroll()
            .bg(theme.bg_surface)
            .border_b_1()
            .border_color(theme.border_soft)
            .children(tabs)
            .into_any_element()
    }

    fn render_editor(&self, theme: Theme) -> impl IntoElement + use<> {
        let content = match self.ide().and_then(Ide::active_file) {
            Some(tab) => tab.editor.clone().into_any_element(),
            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.ink_muted)
                .child("Ouvrez un fichier dans l'explorateur.")
                .into_any_element(),
        };
        div().flex_1().min_h_0().bg(theme.code_bg).child(content)
    }

    fn render_status_bar(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let ide = self.ide();
        let notice = ide.and_then(|ide| ide.notice.as_ref());
        let tab = ide.and_then(Ide::active_file);

        let mut left = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(metrics::SPACE_3);
        // Position du curseur : dessinée par l'éditeur lui-même (voir CodeArea).
        let dirty = tab.is_some_and(|tab| tab.editor.read(cx).is_dirty());
        if let Some(notice) = notice {
            left = left.child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(if notice.error {
                        theme.danger
                    } else {
                        theme.success
                    })
                    .child(notice.text.clone()),
            );
        }

        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(metrics::SPACE_3)
            .h(metrics::CONTROL_H + metrics::SPACE_2)
            .px(metrics::SPACE_3)
            .bg(theme.bg_surface)
            .border_t_1()
            .border_color(theme.border_soft)
            .text_size(metrics::TEXT_SM)
            .text_color(theme.ink_muted)
            .child(left)
            .when(tab.is_some(), |this| {
                this.child(
                    div()
                        .flex_none()
                        .text_color(theme.ink_soft)
                        .child("⌘S enregistrer · ⌘Z annuler · ⌘/ commenter"),
                )
                .child(
                    Button::new("ide-save", "Enregistrer", theme)
                        .variant(if dirty {
                            ButtonVariant::Default
                        } else {
                            ButtonVariant::Ghost
                        })
                        .on_click(cx.listener(|app, _, _, cx| app.save_active_file(cx))),
                )
            })
    }

    /// Problèmes du fichier ouvert ; un clic place le curseur sur la ligne.
    fn render_problems(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tab = self.ide().and_then(Ide::active_file);
        let mut diagnostics: Vec<Diagnostic> = tab
            .map(|tab| tab.editor.read(cx).diagnostics().to_vec())
            .unwrap_or_default();
        diagnostics.sort_by_key(|d| (d.row, d.severity));
        let editor = tab.map(|tab| tab.editor.clone());
        let title = match tab {
            Some(tab) => format!(
                "Problèmes · {} · {}",
                tab.relative,
                count_label(diagnostics.len() as u32, "problème", "problèmes")
            ),
            None => "Problèmes".to_owned(),
        };

        let rows = diagnostics
            .into_iter()
            .enumerate()
            .map(|(ix, diagnostic)| {
                let editor = editor.clone();
                let line = diagnostic.row + 1;
                div()
                    .id(("ide-problem", ix))
                    .flex()
                    .items_center()
                    .gap(metrics::SPACE_3)
                    .px(metrics::SPACE_3)
                    .py(metrics::SPACE_1)
                    .cursor_pointer()
                    .hover(|style| style.bg(theme.bg_hover))
                    .on_click(cx.listener(move |_, _, window, cx| {
                        if let Some(editor) = &editor {
                            editor.update(cx, |area, cx| area.go_to_line(line, cx));
                            let handle = editor.read(cx).focus_handle(cx);
                            window.focus(&handle);
                        }
                    }))
                    .child(
                        div()
                            .w(SEVERITY_WIDTH)
                            .flex_none()
                            .child(SeverityLabel::new(diagnostic.severity, theme)),
                    )
                    .child(
                        div()
                            .w(LINE_LABEL_WIDTH)
                            .flex_none()
                            .font_family(fonts::MONO)
                            .text_size(metrics::TEXT_SM)
                            .text_color(theme.ink_soft)
                            .child(format!("L{line}")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(metrics::TEXT_SM)
                            .child(diagnostic.message),
                    )
            })
            .collect::<Vec<_>>();
        let empty = rows.is_empty();

        div()
            .flex_none()
            .h(PROBLEMS_HEIGHT)
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(theme.border_soft)
            .child(
                div()
                    .flex_none()
                    .px(metrics::SPACE_3)
                    .py(metrics::SPACE_2)
                    .text_size(metrics::TEXT_XS)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.ink_soft)
                    .child(title),
            )
            .child(
                div()
                    .id("ide-problems")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows)
                    .when(empty && tab.is_some(), |this| {
                        this.child(
                            div()
                                .px(metrics::SPACE_3)
                                .text_size(metrics::TEXT_SM)
                                .text_color(theme.ink_muted)
                                .child(
                                    "Aucun problème relevé dans ce fichier à la dernière analyse.",
                                ),
                        )
                    }),
            )
    }
}

/// Une ligne de l'explorateur : guides d'indentation, chevron, nom, problèmes.
fn explorer_row(
    ix: usize,
    row: &ExplorerRow,
    active: bool,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> AnyElement {
    let path = row.path.clone();
    let is_dir = row.is_dir;
    // Un guide par niveau parent, aligné sous le chevron de ce parent.
    let guides = (0..row.depth).map(|level| {
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(TREE_LEFT + TREE_INDENT * level as f32 + CHEVRON_WIDTH / 2.)
            .w(GUIDE_WIDTH)
            .bg(theme.border_soft)
    });
    let chevron = is_dir.then(|| {
        Icon::new(
            if row.open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            },
            theme.ink_soft,
        )
        .size(CHEVRON_ICON)
    });
    let problems = row.count.map(|(count, severity)| {
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(metrics::SPACE_1)
            .text_size(metrics::TEXT_XS)
            .font_weight(FontWeight::MEDIUM)
            .text_color(if is_dir {
                theme.ink_soft
            } else {
                severity_color(severity, theme)
            })
            .when(!is_dir, |this| {
                this.child(
                    div()
                        .size(metrics::DOT)
                        .rounded_full()
                        .bg(severity_color(severity, theme)),
                )
            })
            .child(count.to_string())
    });

    div()
        .id(("tree-row", ix))
        .relative()
        .flex()
        .items_center()
        .gap(metrics::SPACE_1)
        .h(metrics::CONTROL_H - metrics::SPACE_1)
        .pl(TREE_LEFT + TREE_INDENT * row.depth as f32)
        .pr(metrics::SPACE_3)
        .cursor_pointer()
        .text_size(metrics::TEXT_SM)
        .when(active, |this| this.bg(theme.bg_active))
        .when(!active, |this| this.hover(|style| style.bg(theme.bg_hover)))
        .on_click(cx.listener(move |app, _, window, cx| {
            if is_dir {
                app.toggle_dir(path.clone(), cx);
            } else {
                app.open_in_ide(&path, None, window, cx);
            }
        }))
        .children(guides)
        .child(
            div()
                .w(CHEVRON_WIDTH)
                .flex_none()
                .flex()
                .justify_center()
                .children(chevron),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .when(is_dir, |this| this.font_weight(FontWeight::MEDIUM))
                .when(!is_dir && !active, |this| this.text_color(theme.ink_muted))
                .when(active, |this| this.font_weight(FontWeight::MEDIUM))
                .child(row.label.clone()),
        )
        .children(problems)
        .into_any_element()
}

/// Tous / Problèmes.
fn explorer_filter(
    only_problems: bool,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> impl IntoElement {
    let item = |id: &'static str, label: &'static str, value: bool| {
        let active = only_problems == value;
        div()
            .id(id)
            .flex()
            .items_center()
            .h_full()
            .px(metrics::SPACE_2)
            .cursor_pointer()
            .text_size(metrics::TEXT_XS)
            .when(active, |this| {
                this.bg(theme.bg_active).font_weight(FontWeight::MEDIUM)
            })
            .when(!active, |this| {
                this.text_color(theme.ink_muted)
                    .hover(|style| style.bg(theme.bg_hover))
            })
            .on_click(cx.listener(move |app, _, _, cx| app.set_only_problems(value, cx)))
            .child(label)
    };
    div()
        .flex()
        .flex_none()
        .h(metrics::CONTROL_H - metrics::SPACE_1)
        .rounded(metrics::RADIUS)
        .overflow_hidden()
        .border_1()
        .border_color(theme.border)
        .child(item("explorer-all", "Tous", false))
        .child(div().w(GUIDE_WIDTH).h_full().bg(theme.border))
        .child(item("explorer-problems", "Problèmes", true))
}

/// Lit un fichier texte pour l'éditeur.
fn read_text(path: &Path) -> Result<String, String> {
    let size = std::fs::metadata(path)
        .map_err(|err| format!("Ouverture impossible : {err}"))?
        .len();
    if size > MAX_OPEN_BYTES {
        return Err("Fichier trop volumineux pour l'éditeur (plus de 2 Mo).".to_owned());
    }
    let bytes = std::fs::read(path).map_err(|err| format!("Lecture impossible : {err}"))?;
    String::from_utf8(bytes)
        .map_err(|_| "Fichier binaire : il ne s'ouvre pas dans l'éditeur.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_sorted_tree_with_folders_first() {
        let tree = build_tree(&[
            "src/main.rs".to_owned(),
            "README.md".to_owned(),
            "src/ui/app.rs".to_owned(),
            "Cargo.toml".to_owned(),
        ]);
        let names: Vec<&str> = tree.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["src", "Cargo.toml", "README.md"]);
        let src = &tree[0];
        assert_eq!(src.children[0].path, "src/ui");
        assert_eq!(src.children[1].path, "src/main.rs");
        assert!(tree_contains_dir(&tree, "src/ui"));
        assert!(!tree_contains_dir(&tree, "src/main.rs"));
    }

    fn labels(rows: &[ExplorerRow]) -> Vec<(String, usize)> {
        rows.iter().map(|r| (r.label.clone(), r.depth)).collect()
    }

    #[test]
    fn single_child_folders_are_compacted() {
        let tree = build_tree(&[
            "src/main/java/App.java".to_owned(),
            "src/main/java/Util.java".to_owned(),
            "pom.xml".to_owned(),
        ]);
        let closed = explorer_rows(&tree, &HashSet::new(), &HashMap::new(), false);
        assert_eq!(
            labels(&closed),
            [("src/main/java".to_owned(), 0), ("pom.xml".to_owned(), 0)]
        );
        // Ouvrir la chaîne, c'est ouvrir son dossier le plus profond.
        assert_eq!(closed[0].path, "src/main/java");
        let open: HashSet<String> = ["src/main/java".to_owned()].into();
        let rows = explorer_rows(&tree, &open, &HashMap::new(), false);
        assert_eq!(
            labels(&rows),
            [
                ("src/main/java".to_owned(), 0),
                ("App.java".to_owned(), 1),
                ("Util.java".to_owned(), 1),
                ("pom.xml".to_owned(), 0),
            ]
        );
    }

    #[test]
    fn problems_filter_keeps_only_files_with_issues() {
        let tree = build_tree(&[
            "src/a.rs".to_owned(),
            "src/b.rs".to_owned(),
            "lib/c.rs".to_owned(),
        ]);
        let counts: HashMap<String, (usize, Severity)> = [
            ("src".to_owned(), (2, Severity::Critical)),
            ("src/a.rs".to_owned(), (2, Severity::Critical)),
        ]
        .into();
        let open: HashSet<String> = ["src".to_owned(), "lib".to_owned()].into();
        let rows = explorer_rows(&tree, &open, &counts, true);
        assert_eq!(
            labels(&rows),
            [("src".to_owned(), 0), ("a.rs".to_owned(), 1)]
        );
        assert_eq!(rows[1].count, Some((2, Severity::Critical)));
    }
}

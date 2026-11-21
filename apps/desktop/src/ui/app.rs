use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{
    ExternalPaths, PathPromptOptions, Pixels, PromptButton, PromptLevel, SharedString, Window,
    WindowAppearance, div, prelude::*, px,
};

use super::analysis_run::AnalysisRun;
use super::ide::Ide;
use super::new_project::NewProjectModal;
use crate::engine::cve;
use crate::model::{
    self, Advisory, AnalysisReport, Category, Grade, IssueGroup, Project, RuleCatalog,
};
use crate::store;
use crate::theme::{Theme, ThemeMode, fonts, metrics};

/// Clé du réglage d'apparence.
pub(super) const THEME_MODE: &str = "theme_mode";

/// Écran affiché dans la fenêtre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Screen {
    /// Vue d'ensemble : une carte par projet.
    Home,
    /// Page d'un projet (note, catégories, gains rapides).
    Project,
    Rules,
    Analysis,
    Alerts,
    /// Compte, analyse, données locales.
    Settings,
}

/// Présentation de l'écran d'analyse : liste des problèmes ou IDE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WorkspaceView {
    /// Vue générale : une boîte par catégorie.
    General,
    /// Plan de correction.
    List,
    /// Patterns de conception : une boîte par pattern, avec des astuces.
    Patterns,
    /// Éditeur.
    Code,
    /// Paquets du projet et leurs vulnérabilités connues.
    Dependencies,
}

/// Alertes CVE (onglet et bandeau de l'accueil) : vraies vulnérabilités OSV des
/// dépendances des projets.
pub(super) const ALERTS_ENABLED: bool = true;

/// Vue racine : détient l'état de navigation et de sélection.
///
/// Le rendu de chaque écran est implémenté dans `home.rs`, `rules.rs`, `analysis.rs` et `alerts.rs`.
pub struct HeraldApp {
    pub(super) projects: Vec<Project>,
    pub(super) selected_project: usize,
    pub(super) screen: Screen,
    /// Groupe sélectionné (indice dans `AnalysisReport::groups`).
    pub(super) selected_group: Option<usize>,
    /// Occurrence affichée dans la fiche (indice dans `IssueGroup::occurrences`).
    pub(super) selected_occurrence: usize,
    /// Hauteur de la fenêtre, pour dimensionner la liste et la fiche.
    pub(super) viewport_height: Pixels,
    /// Largeur de la fenêtre (mise en page étroite).
    pub(super) viewport_width: Pixels,
    /// Catégorie affichée sur l'écran d'analyse ; `None` pour toutes.
    pub(super) category_filter: Option<Category>,
    /// Vulnérabilités publiées qui touchent les dépendances des projets.
    pub(super) alerts: Vec<Advisory>,
    /// Filtres et page de l'écran des alertes CVE.
    pub(super) alert_filter: super::alerts::AlertFilter,
    /// Dépendances de chaque projet, relevées à la dernière analyse.
    pub(super) dependencies: HashMap<PathBuf, Vec<cve::Dependency>>,
    /// Historique des notes de chaque projet, la plus récente d'abord
    /// (courbe de l'onglet Général) ; mis à jour après chaque analyse.
    pub(super) score_history: HashMap<PathBuf, Vec<store::HistoryEntry>>,
    /// Fichiers analysés de chaque projet (carte du code de l'onglet Général),
    /// relus du disque une fois par analyse (clé : date de l'analyse).
    pub(super) code_maps: std::cell::RefCell<HashMap<PathBuf, (u64, Vec<String>)>>,
    /// Onglet « Dépendances ».
    pub(super) dependency_tab: super::dependencies::DependencyTab,
    /// Modale d'un paquet de l'onglet Dépendances (problème et correction).
    pub(super) dependency_modal: Option<super::dependency_modal::DependencyModal>,
    /// Panneau « Votre note » réduit à une bande (choix mémorisé).
    pub(super) score_panel_collapsed: bool,
    /// Thème choisi dans les Réglages.
    pub(super) theme_mode: ThemeMode,
    /// Suit l'apparence de macOS (thème « Système »).
    _appearance: Option<gpui::Subscription>,
    /// Onglet Code : la carte du code au lieu de l'éditeur.
    pub(super) code_map_open: bool,
    /// Note atteinte grâce aux dernières corrections, à célébrer.
    pub(super) celebration: Option<Grade>,
    /// Modale « Nouveau projet », si elle est ouverte.
    pub(super) new_project: Option<NewProjectModal>,
    /// Analyse en cours, affichée en plein écran.
    pub(super) analysis_run: Option<AnalysisRun>,
    /// Règles d'analyse, chargées à la première ouverture de l'écran « Règles ».
    pub(super) workspace_view: WorkspaceView,
    /// Pattern dont la page est ouverte dans l'onglet Patterns (son nom) ;
    /// `None` : la grille de tous les patterns.
    pub(super) open_pattern: Option<&'static str>,
    /// IDE de chaque projet ouvert (onglets gardés quand on change de projet).
    pub(super) ides: HashMap<PathBuf, Ide>,
    pub(super) rules: Option<RuleCatalog>,
    pub(super) rules_loading: bool,
    /// Catégorie affichée sur l'écran « Règles » ; `None` pour toutes.
    pub(super) rule_filter: Option<Category>,
    /// Page affichée de la liste des règles officielles (à partir de 0).
    pub(super) rule_page: usize,
    /// Règle dont la source est dépliée.
    pub(super) expanded_rule: Option<SharedString>,
    /// Titre courant de la fenêtre (« Async Herald — Problèmes »).
    window_title: String,
    /// Écran « Réglages ».
    pub(super) settings: super::settings::SettingsState,
}

impl HeraldApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut app = Self {
            projects: store::load_projects().unwrap_or_else(|| {
                // Premier lancement : dossiers proposés, sauvegardés tout de suite.
                let projects = model::sample::projects();
                store::save_projects(&projects);
                projects
            }),
            selected_project: 0,
            screen: Screen::Home,
            selected_group: None,
            selected_occurrence: 0,
            viewport_height: px(860.),
            viewport_width: px(1360.),
            category_filter: None,
            alerts: store::load_alerts(),
            alert_filter: Default::default(),
            dependencies: store::all_dependencies(),
            score_history: HashMap::new(),
            code_maps: Default::default(),
            dependency_tab: Default::default(),
            dependency_modal: None,
            score_panel_collapsed: store::setting(super::plan::SCORE_PANEL_COLLAPSED, false),
            theme_mode: ThemeMode::from_key(&store::text_setting(THEME_MODE).unwrap_or_default()),
            _appearance: None,
            code_map_open: false,
            celebration: None,
            new_project: None,
            analysis_run: None,
            workspace_view: WorkspaceView::General,
            open_pattern: None,
            ides: HashMap::new(),
            rules: None,
            rules_loading: false,
            rule_filter: None,
            rule_page: 0,
            expanded_rule: None,
            window_title: String::new(),
            settings: Default::default(),
        };
        app.score_history = app
            .projects
            .iter()
            .map(|project| (project.path.clone(), store::history(&project.path)))
            .collect();
        app.reset_selection();
        app.refresh_alerts(cx);
        app.start_service(window, cx);
        // `--open <dossier>` (extension VS Code) : ajoute et affiche ce projet.
        if let Some(path) = crate::headless::requested_open() {
            app.add_project(path, cx);
        }
        // Développement : `HERALD_DEV_THEME=dark|light|system` (non enregistré).
        #[cfg(debug_assertions)]
        if let Ok(key) = std::env::var("HERALD_DEV_THEME") {
            app.theme_mode = ThemeMode::from_key(&key);
        }
        // Thème « Système » : redessine quand macOS change d'apparence.
        app._appearance = Some(cx.observe_window_appearance(window, |_, _, cx| cx.notify()));
        app
    }

    /// Revérifie au démarrage les dépendances connues de chaque projet auprès
    /// d'OSV : une CVE publiée depuis la dernière analyse apparaît sans relancer.
    fn refresh_alerts(&mut self, cx: &mut Context<Self>) {
        if !store::setting(super::settings::CVE_ON_START, true) {
            return;
        }
        let projects: Vec<(PathBuf, String)> = self
            .projects
            .iter()
            .map(|project| (project.path.clone(), project.name.to_string()))
            .collect();
        cx.spawn(async move |this, cx| {
            let changed = cx
                .background_executor()
                .spawn(async move {
                    let mut deps = store::all_dependencies();
                    let mut changed = false;
                    for (path, name) in projects {
                        let Some(deps) = deps.remove(&path) else {
                            continue;
                        };
                        match cve::scan(&name, &path, &deps) {
                            Ok(advisories) => {
                                store::replace_alerts(&path, &advisories);
                                changed = true;
                            }
                            Err(err) => {
                                eprintln!("[cve] {err}");
                                break;
                            }
                        }
                    }
                    changed
                })
                .await;
            if changed {
                this.update(cx, |app, cx| {
                    app.alerts = store::load_alerts();
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    pub(super) fn project(&self) -> Option<&Project> {
        self.projects.get(self.selected_project)
    }

    pub(super) fn report(&self) -> Option<&AnalysisReport> {
        self.project().and_then(|project| project.report.as_ref())
    }

    pub(super) fn selected_group(&self) -> Option<&IssueGroup> {
        self.report()?.group(self.selected_group?)
    }

    /// Indice (dans `AnalysisReport::issues`) de l'occurrence affichée dans la fiche.
    pub(super) fn selected_issue_index(&self) -> Option<usize> {
        let group = self.selected_group()?;
        group
            .occurrences
            .get(self.selected_occurrence)
            .or(group.occurrences.first())
            .copied()
    }

    /// Groupes visibles avec le filtre courant, les plus graves d'abord.
    pub(super) fn visible_group_indices(&self) -> Vec<usize> {
        self.report()
            .map(|report| report.group_indices(self.category_filter))
            .unwrap_or_default()
    }

    /// Alertes non lues dont la version installée est connue : une faille « à
    /// confirmer » (version déclarée seulement) ne fait pas monter le compteur.
    pub(super) fn unread_alert_count(&self) -> usize {
        self.alerts
            .iter()
            .filter(|alert| !alert.read && alert.confirmed)
            .count()
    }

    /// Sélectionne le problème le plus grave du projet, sans filtre.
    pub(super) fn reset_selection(&mut self) {
        self.category_filter = None;
        self.celebration = None;
        self.selected_group = self.visible_group_indices().first().copied();
        self.selected_occurrence = 0;
        self.open_pattern = None;
    }

    pub(super) fn select_project(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.projects.len() || ix == self.selected_project {
            return;
        }
        self.selected_project = ix;
        self.reset_selection();
        if self.report().is_none() || matches!(self.screen, Screen::Home | Screen::Project) {
            self.screen = self.project_screen();
        }
        cx.notify();
    }

    /// Écran d'un projet : ses problèmes s'il a été analysé, sinon sa page
    /// (pour lancer la première analyse).
    pub(super) fn project_screen(&self) -> Screen {
        if self.report().is_some() {
            Screen::Analysis
        } else {
            Screen::Project
        }
    }

    pub(super) fn open_analysis(&mut self, cx: &mut Context<Self>) {
        if self.report().is_none() {
            return;
        }
        self.screen = Screen::Analysis;
        cx.notify();
    }

    pub(super) fn go_home(&mut self, cx: &mut Context<Self>) {
        self.screen = Screen::Home;
        cx.notify();
    }

    pub(super) fn select_group(&mut self, gi: usize, cx: &mut Context<Self>) {
        if self
            .report()
            .is_some_and(|report| gi < report.groups().len())
            && self.selected_group != Some(gi)
        {
            self.selected_group = Some(gi);
            self.selected_occurrence = 0;
            cx.notify();
        }
    }

    /// Marque ou démarque un groupe comme corrigé et fête un changement de note.
    pub(super) fn toggle_group_resolved(&mut self, gi: usize, cx: &mut Context<Self>) {
        let Some(report) = self
            .projects
            .get_mut(self.selected_project)
            .and_then(|project| project.report.as_mut())
        else {
            return;
        };
        let before = Grade::from_score(report.projected_score());
        report.toggle_group_resolved(gi);
        let after = Grade::from_score(report.projected_score());
        if let Some(project) = self.projects.get(self.selected_project) {
            if let Some(report) = &project.report {
                store::save_resolved(&project.path, report);
            }
        }
        self.celebration = after.is_better_than(before).then_some(after);
        cx.notify();
    }

    pub(super) fn dismiss_celebration(&mut self, cx: &mut Context<Self>) {
        self.celebration = None;
        cx.notify();
    }

    pub(super) fn open_alerts(&mut self, cx: &mut Context<Self>) {
        self.screen = Screen::Alerts;
        cx.notify();
    }

    pub(super) fn mark_alert_read(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(alert) = self.alerts.get_mut(ix) {
            alert.read = true;
            store::set_alert_read(alert);
            cx.notify();
        }
    }

    pub(super) fn mark_all_alerts_read(&mut self, cx: &mut Context<Self>) {
        for alert in &mut self.alerts {
            alert.read = true;
        }
        store::set_all_alerts_read();
        cx.notify();
    }

    /// Ouvre le projet touché par une alerte et la marque comme lue.
    pub(super) fn open_alert_project(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(alert) = self.alerts.get_mut(ix) else {
            return;
        };
        alert.read = true;
        store::set_alert_read(alert);
        let path = alert.project_path.clone();
        if let Some(project_ix) = self
            .projects
            .iter()
            .position(|project| project.path == path)
        {
            if project_ix != self.selected_project {
                self.selected_project = project_ix;
                self.reset_selection();
            }
        }
        self.screen = if self.report().is_some() {
            Screen::Analysis
        } else {
            Screen::Project
        };
        cx.notify();
    }

    /// Demande confirmation avant de retirer un projet de l'app.
    pub(super) fn confirm_remove_project(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.projects.get(ix) else {
            return;
        };
        let path = project.path.clone();
        let message = format!("Retirer « {} » de Herald ?", project.name);
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some(
                "Le dossier et ses fichiers restent sur votre disque. \
                 Seuls le projet et ses résultats d'analyse sont retirés de l'app.",
            ),
            &[PromptButton::ok("Retirer"), PromptButton::cancel("Annuler")],
            cx,
        );

        cx.spawn(async move |this, cx| {
            if answer.await == Ok(0) {
                this.update(cx, |app, cx| app.remove_project(&path, cx))
                    .ok();
            }
        })
        .detach();
    }

    /// Retire un projet de la liste (sans toucher au dossier sur le disque).
    fn remove_project(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(ix) = self
            .projects
            .iter()
            .position(|project| project.path == path)
        else {
            return;
        };
        let removed = self.projects.remove(ix);
        self.forget_ide(&removed.path);
        store::save_projects(&self.projects);
        self.alerts
            .retain(|alert| alert.project_path != removed.path);
        self.dependencies.remove(&removed.path);

        if ix < self.selected_project {
            // Même projet sélectionné, décalé d'un cran.
            self.selected_project -= 1;
        } else if ix == self.selected_project {
            self.selected_project = ix.min(self.projects.len().saturating_sub(1));
            self.reset_selection();
        }
        if self.report().is_none() && self.screen != Screen::Home {
            self.screen = Screen::Project;
        }
        cx.notify();
    }

    /// Ouvre le sélecteur de dossier natif et ajoute le dossier choisi.
    pub(super) fn pick_folder(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Analyser".into()),
        });

        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut paths))) = receiver.await else {
                return;
            };
            if let Some(path) = paths.pop() {
                this.update(cx, |app, cx| app.add_project(path, cx)).ok();
            }
        })
        .detach();
    }

    /// Ajoute le premier dossier déposé sur la zone de dépôt.
    pub(super) fn add_dropped_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        if let Some(dir) = paths.iter().find(|path| path.is_dir()) {
            self.add_project(dir.clone(), cx);
        }
    }

    pub(super) fn add_project(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.add_existing_project(Project::from_path(path), cx);
    }

    /// Ajoute un projet (ou sélectionne celui qui a le même dossier) et l'affiche.
    pub(super) fn add_existing_project(&mut self, project: Project, cx: &mut Context<Self>) {
        let ix = match self
            .projects
            .iter()
            .position(|existing| existing.path == project.path)
        {
            Some(ix) => ix,
            None => {
                self.projects.push(project);
                store::save_projects(&self.projects);
                self.projects.len() - 1
            }
        };
        self.selected_project = ix;
        self.screen = Screen::Project;
        self.reset_selection();
        cx.notify();
    }
}

impl Render for HeraldApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Apparence : choix des Réglages, ou celle de macOS.
        let dark = match self.theme_mode {
            ThemeMode::Light => false,
            ThemeMode::Dark => true,
            ThemeMode::System => matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
        };
        crate::theme::set_dark(dark);
        let theme = Theme::current();
        self.viewport_height = window.viewport_size().height;
        self.viewport_width = window.viewport_size().width;
        let root = div()
            .size_full()
            .font_family(fonts::SANS)
            .text_size(metrics::TEXT_BASE)
            .bg(theme.bg_page)
            .text_color(theme.ink);
        // Version officielle sans session : pré-page de connexion.
        if let Some(gate) = self.render_gate(theme, window, cx) {
            return root.relative().child(gate);
        }

        if self.screen == Screen::Analysis && self.workspace_view == WorkspaceView::Code {
            self.ensure_ide();
        }
        let (scroll_id, content) = match self.screen {
            Screen::Home => ("home", self.render_overview(theme, cx)),
            Screen::Project => ("project", self.render_project_page(theme, cx)),
            Screen::Rules => ("rules", self.render_rules(theme, cx)),
            Screen::Analysis => ("analysis", self.render_analysis(theme, cx)),
            Screen::Alerts => ("alerts", self.render_alerts(theme, cx)),
            Screen::Settings => ("settings", self.render_settings(theme, cx)),
        };

        let title = format!(
            "Async Herald — {}",
            match self.screen {
                Screen::Home => "Vue d'ensemble".to_owned(),
                Screen::Project => self
                    .project()
                    .map_or_else(|| "Projet".to_owned(), |p| p.name.to_string()),
                Screen::Rules => "Règles".to_owned(),
                Screen::Analysis if self.workspace_view == WorkspaceView::Dependencies => {
                    "Dépendances".to_owned()
                }
                Screen::Analysis => "Problèmes".to_owned(),
                Screen::Alerts => "Alertes CVE".to_owned(),
                Screen::Settings => "Réglages".to_owned(),
            }
        );
        if self.window_title != title {
            window.set_window_title(&title);
            self.window_title = title;
        }
        // Écran d'un projet : la vue et le panneau de la note occupent toute la
        // place ; chaque vue gère ses marges et son défilement.
        let full_bleed = self.screen == Screen::Analysis;

        let new_project = self.render_new_project(theme, cx);
        let dependency_modal = self.render_dependency_modal(theme, cx);
        let update_banner = self.render_update_banner(theme, cx);
        let update_required = self.render_update_required(theme, cx);
        let version = self.render_version(theme, cx);
        root.relative()
            .flex()
            // Un dossier peut être déposé n'importe où dans la fenêtre.
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(theme.bg_active))
            .on_drop(cx.listener(|app, paths: &ExternalPaths, _, cx| {
                app.add_dropped_paths(paths.paths(), cx);
            }))
            .child(self.render_sidebar(theme, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    // Nouvelle version disponible : bandeau au-dessus de l'écran.
                    .children(update_banner)
                    .child(
                        div()
                            .id(SharedString::new_static(scroll_id))
                            .flex_1()
                            .min_h_0()
                            .when(!full_bleed, |this| {
                                this.overflow_y_scroll()
                                    .px(metrics::SPACE_8)
                                    .py(metrics::SPACE_6)
                            })
                            .child(content),
                    )
                    // Analyse en cours : mémoire et processeur de Herald.
                    .children(self.render_resource_bar(theme)),
            )
            .children(new_project)
            .children(dependency_modal)
            .child(version)
            .children(update_required)
    }
}

//! Analyse d'un projet, affichée discrètement en haut de la page du projet (pas
//! d'écran dédié) : une carte avec l'étape réelle du moteur, les fichiers lus,
//! la barre de progression et le dernier fichier lu ; l'app reste utilisable
//! pendant l'analyse.
//!
//! Les étapes sont réelles (signalées par le moteur). Le moteur ne
//! dit pas quel fichier il traite : pendant l'étape « Analyse », le fichier affiché
//! suit une durée estimée d'après le nombre de fichiers. À l'arrivée du résultat,
//! le rapport remplace l'ancien sur place et la carte affiche un résumé.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationExt, FontWeight, Hsla, PathBuilder, Pixels, SharedString, Task,
    Transformation, canvas, div, point, prelude::*, px, radians, svg,
};

use super::app::HeraldApp;
use super::components::{Button, ButtonVariant, Icon, IconName, ProgressBar};
use super::home::{count_label, surface};
use super::resource_monitor::{self, ResourceTrace};
use super::service::Quota;
use crate::engine::{self, Analysis, Phase, SharedPhase, cve};
use crate::model::{Grade, Severity};
use crate::store;
use crate::theme::{Theme, fonts, metrics};

/// La jauge ne monte jamais seule jusqu'à 100 (comme sur le site).
const PROGRESS_CAP: f32 = 92.;
const TICK: Duration = Duration::from_millis(60);
const MIN_ESTIMATE_MS: f32 = 4_000.;
const MAX_ESTIMATE_MS: f32 = 90_000.;

const GAUGE_SIZE: Pixels = px(56.);
const GAUGE_STROKE: Pixels = px(6.);
const SPINNER_SIZE: Pixels = px(12.);
/// Étapes de l'analyse (voir `STEP_NAMES`).
const STEPS: usize = 6;

/// Avancement d'une analyse, lu par la vue d'ensemble.
pub(super) struct RunSnapshot {
    pub(super) percent: u32,
    pub(super) step: String,
    pub(super) file: Option<SharedString>,
}

/// Résumé affiché une fois l'analyse terminée.
struct Outcome {
    grade: Grade,
    score: u8,
    issues: usize,
    critical: usize,
}

/// Analyse en cours (ou tout juste terminée) d'un projet.
pub(super) struct AnalysisRun {
    pub(super) path: PathBuf,
    phase: SharedPhase,
    files: Vec<SharedString>,
    /// Indice dans `files` du fichier affiché.
    current: Option<usize>,
    step_started: [Option<Instant>; STEPS],
    step_took: [Option<Duration>; STEPS],
    percent: u32,
    rules: Option<usize>,
    outcome: Option<Outcome>,
    failure: Option<SharedString>,
    /// Mémoire et processeur de Herald pendant l'analyse (barre du bas).
    pub(super) resources: ResourceTrace,
    _analysis: Task<()>,
    _ticker: Task<()>,
    _monitor: Task<()>,
}

impl AnalysisRun {
    /// Analyse terminée (avec un résultat ou un échec).
    pub(super) fn is_over(&self) -> bool {
        self.outcome.is_some() || self.failure.is_some()
    }
}

impl HeraldApp {
    /// Avancement de l'analyse en cours de `path`, pour les cartes de la vue d'ensemble.
    pub(super) fn run_snapshot(&self, path: &std::path::Path) -> Option<RunSnapshot> {
        let run = self.analysis_run.as_ref()?;
        if run.path != path || run.is_over() {
            return None;
        }
        Some(RunSnapshot {
            percent: run.percent.min(100),
            step: run.phase_label(),
            file: run.current.and_then(|ix| run.files.get(ix)).cloned(),
        })
    }

    /// `true` si `path` est en cours d'analyse (pour la barre latérale).
    pub(super) fn is_analyzing(&self, path: &std::path::Path) -> bool {
        self.analysis_run
            .as_ref()
            .is_some_and(|run| run.path == path && !run.is_over())
    }

    /// Sélectionne le projet `ix` et lance son analyse. `show` : ouvre sa page
    /// (barre latérale) ; sinon on reste où l'on est (cartes de la vue d'ensemble).
    pub(super) fn analyze_project_at(&mut self, ix: usize, show: bool, cx: &mut Context<Self>) {
        if ix >= self.projects.len() {
            return;
        }
        if ix != self.selected_project {
            self.selected_project = ix;
            self.reset_selection();
        }
        if show {
            self.screen = self.project_screen();
        }
        self.start_analysis(cx);
    }

    /// Lance l'analyse du projet sélectionné.
    pub(super) fn start_analysis(&mut self, cx: &mut Context<Self>) {
        if self.analysis_run.as_ref().is_some_and(|run| !run.is_over()) {
            return;
        }
        // Quota du jour atteint : la barre latérale l'indique, rien à lancer.
        if self.usage().is_some_and(|usage| usage.is_exhausted()) {
            return;
        }
        // Toute première analyse : demander d'abord pour le fil public.
        if self.ask_public_feed_first(cx) {
            return;
        }
        let Some(project) = self.project() else {
            return;
        };
        let path = project.path.clone();
        let remote = project.remote.clone();
        let previous_score = project.report.as_ref().map(|report| report.summary.score);
        let phase = SharedPhase::default();

        let engine_phase = Arc::clone(&phase);
        let engine_path = path.clone();
        let service = self.analysis_service();
        let analysis = cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    // Version officielle : quota décompté AVANT l'analyse, comme sur le web.
                    let quota = service.reserve()?;
                    let analysis = engine::analyze_project(
                        engine::Request {
                            path: &engine_path,
                            remote: remote.as_deref(),
                            fetch_rules: true,
                            previous_score,
                        },
                        &engine_phase,
                    )
                    .map_err(RunError::Analysis)?;
                    Ok((analysis, quota))
                })
                .await;
            this.update(cx, |app, cx| app.complete_analysis(result, cx))
                .ok();
        });

        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let over = this
                    .update(cx, |app, cx| {
                        let Some(run) = &mut app.analysis_run else {
                            return true;
                        };
                        if run.is_over() {
                            return true;
                        }
                        run.advance();
                        cx.notify();
                        false
                    })
                    .unwrap_or(true);
                if over {
                    break;
                }
            }
        });

        let mut step_started = [None; STEPS];
        step_started[0] = Some(Instant::now());
        self.analysis_run = Some(AnalysisRun {
            path,
            phase,
            files: Vec::new(),
            current: None,
            step_started,
            step_took: [None; STEPS],
            percent: 0,
            rules: None,
            outcome: None,
            failure: None,
            resources: ResourceTrace::new(),
            _analysis: analysis,
            _ticker: ticker,
            _monitor: resource_monitor::spawn_monitor(cx),
        });
        cx.notify();
    }

    /// Range le rapport dans le projet, sur place, et résume le résultat.
    fn complete_analysis(
        &mut self,
        result: Result<(Analysis, Option<Quota>), RunError>,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = &mut self.analysis_run else {
            return;
        };
        let now = Instant::now();
        run.resources.finish();
        match result {
            Ok((
                Analysis {
                    report,
                    dependencies,
                },
                quota,
            )) => {
                run.enter_step(STEPS - 1, now);
                let last = run.step_started[STEPS - 1].unwrap_or(now);
                run.step_took[STEPS - 1] = Some(now - last);
                run.percent = 100;
                run.current = None;
                run.outcome = Some(Outcome {
                    grade: report.summary.grade(),
                    score: report.summary.score,
                    issues: report.issues.len(),
                    critical: report
                        .issues
                        .iter()
                        .filter(|issue| issue.severity == Severity::Critical)
                        .count(),
                });
                let path = run.path.clone();
                if let Some(quota) = quota {
                    self.apply_quota(quota);
                }
                if let Some(ix) = self.projects.iter().position(|p| p.path == path) {
                    // Sauvegarde locale : rapport (sans le code) et dépendances.
                    let local_id = store::save_analysis(&path, &report);
                    self.score_history
                        .insert(path.clone(), store::history(&path));
                    store::save_dependencies(&path, &dependencies);
                    self.dependencies.insert(path.clone(), dependencies.clone());
                    let name = self.projects[ix].name.to_string();
                    let score = report.summary.score;
                    self.projects[ix].report = Some(report);
                    if ix == self.selected_project {
                        self.reset_selection();
                        // Première analyse lancée depuis la page du projet : on
                        // passe directement à ses problèmes.
                        if self.screen == super::app::Screen::Project {
                            self.screen = super::app::Screen::Analysis;
                        }
                    }
                    self.refresh_ide(&path, cx);
                    self.after_analysis(path, name, dependencies, score, local_id, cx);
                    // Développement : `HERALD_DEV_OPEN=general|list|patterns|code|deps` ouvre l'écran Problèmes.
                    #[cfg(debug_assertions)]
                    if let Ok(view) = std::env::var("HERALD_DEV_OPEN") {
                        self.workspace_view = match view.as_str() {
                            "code" => super::app::WorkspaceView::Code,
                            "deps" => super::app::WorkspaceView::Dependencies,
                            "patterns" => super::app::WorkspaceView::Patterns,
                            "general" => super::app::WorkspaceView::General,
                            _ => super::app::WorkspaceView::List,
                        };
                        // `HERALD_DEV_PATTERN=<début du nom>` ouvre la page d'un pattern.
                        if let Ok(name) = std::env::var("HERALD_DEV_PATTERN") {
                            self.open_pattern = crate::model::patterns::PATTERNS
                                .iter()
                                .map(|pattern| pattern.name)
                                .find(|candidate| candidate.starts_with(name.as_str()));
                        }
                        self.analysis_run = None;
                        self.screen = super::app::Screen::Analysis;
                        return cx.notify();
                    }
                }
            }
            Err(RunError::Quota(message)) => {
                run.failure = Some(message.into());
                if let Some(usage) = self.usage_mut() {
                    usage.used = usage.limit.unwrap_or(usage.used);
                }
            }
            Err(RunError::Analysis(message)) => run.failure = Some(message.into()),
        }
        cx.notify();
    }

    /// Après l'analyse, en arrière-plan : enregistre le résumé sur le compte
    /// (version officielle) et cherche les CVE des dépendances dans la base OSV.
    fn after_analysis(
        &mut self,
        path: PathBuf,
        name: String,
        dependencies: Vec<cve::Dependency>,
        score: u8,
        local_id: Option<i64>,
        cx: &mut Context<Self>,
    ) {
        let service = self.analysis_service();
        cx.spawn(async move |this, cx| {
            let (quota, scanned) = cx
                .background_executor()
                .spawn(async move {
                    let quota = service.record(name.clone(), score, local_id);
                    let scanned = cve::scan(&name, &path, &dependencies);
                    if let Ok(advisories) = &scanned {
                        store::replace_alerts(&path, advisories);
                    }
                    (quota, scanned)
                })
                .await;
            this.update(cx, |app, cx| {
                if let Some(quota) = quota {
                    app.apply_quota(quota);
                }
                match scanned {
                    Ok(_) => app.alerts = store::load_alerts(),
                    Err(err) => eprintln!("[cve] {err}"),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn dismiss_analysis(&mut self, cx: &mut Context<Self>) {
        if self.analysis_run.as_ref().is_some_and(AnalysisRun::is_over) {
            self.analysis_run = None;
            cx.notify();
        }
    }

    /// Développement uniquement : `HERALD_DEV_ANALYZE=<dossier>` ajoute le dossier et
    /// lance son analyse dès la connexion (vérifier l'affichage sans cliquer).
    pub(super) fn dev_autostart(&mut self, cx: &mut Context<Self>) {
        #[cfg(debug_assertions)]
        if let Some(path) = std::env::var_os("HERALD_DEV_ANALYZE") {
            self.add_project(PathBuf::from(path), cx);
            self.start_analysis(cx);
        }
        #[cfg(not(debug_assertions))]
        let _ = cx;
    }

    /// Carte d'analyse en haut de la page du projet sélectionné.
    pub(super) fn render_analysis_card(
        &self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let run = self.analysis_run.as_ref()?;
        if self
            .project()
            .is_none_or(|project| project.path != run.path)
        {
            return None;
        }

        let card = surface(theme)
            .flex()
            .items_center()
            .gap(metrics::SPACE_4)
            .px(metrics::SPACE_4)
            .py(metrics::SPACE_3);

        if let Some(message) = &run.failure {
            return Some(
                card.child(Icon::new(IconName::Close, theme.danger))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(format!("L'analyse n'a pas abouti : {message}")),
                    )
                    .child(close_button(theme, cx))
                    .into_any_element(),
            );
        }

        if let Some(outcome) = &run.outcome {
            let color = grade_tone(outcome.score, &theme);
            let plural = |n: usize| if n > 1 { "s" } else { "" };
            let mut detail = format!(
                "{} / 100 · {} problème{}",
                outcome.score,
                outcome.issues,
                plural(outcome.issues)
            );
            if outcome.critical > 0 {
                detail.push_str(&format!(
                    " dont {} critique{}",
                    outcome.critical,
                    plural(outcome.critical)
                ));
            }
            return Some(
                card.child(
                    div()
                        .relative()
                        .size(GAUGE_SIZE)
                        .flex_none()
                        .child(ring(f32::from(outcome.score) / 100., color, theme.track))
                        .child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .font_family(fonts::DISPLAY)
                                .font_weight(FontWeight::BOLD)
                                .text_size(metrics::TEXT_LG)
                                .child(outcome.grade.label()),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(metrics::SPACE_1)
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .child("Analyse terminée"),
                        )
                        .child(
                            div()
                                .text_size(metrics::TEXT_SM)
                                .text_color(theme.ink_muted)
                                .child(detail),
                        ),
                )
                .child(
                    Button::new("analysis-open", "Voir les problèmes", theme)
                        .variant(ButtonVariant::Primary)
                        .on_click(cx.listener(|app, _, _, cx| {
                            app.open_analysis(cx);
                            app.dismiss_analysis(cx);
                        })),
                )
                .child(close_button(theme, cx))
                .into_any_element(),
            );
        }

        // En cours : une carte compacte, avec les chiffres réels du moteur.
        let progress = run.progress();
        let (title, facts, detail) = (progress.title, progress.facts, progress.detail);
        Some(
            card.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(metrics::SPACE_2)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(metrics::SPACE_2)
                            .child(spinner(theme))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(metrics::TEXT_SM)
                                    .text_color(theme.ink_muted)
                                    .child(facts),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(44.))
                                    .flex()
                                    .justify_end()
                                    .font_family(fonts::MONO)
                                    .text_size(metrics::TEXT_SM)
                                    .text_color(theme.ink_muted)
                                    .child(format!("{} %", run.percent)),
                            ),
                    )
                    .child(ProgressBar::new(run.percent as f32 / 100., theme))
                    .child(
                        div()
                            .truncate()
                            .font_family(fonts::MONO)
                            .text_size(metrics::TEXT_SM)
                            .text_color(theme.ink_soft)
                            .child(detail),
                    ),
            )
            .into_any_element(),
        )
    }
}

impl AnalysisRun {
    /// Étape correspondant à l'état du moteur.
    fn step_of(phase: &Phase) -> usize {
        match phase {
            Phase::Preparing => 0,
            Phase::Rules => 1,
            Phase::Reading { .. } => 2,
            Phase::Analyzing { .. } => 3,
            Phase::Dependencies => 4,
            Phase::Scoring => 5,
        }
    }

    /// Passe à l'étape `step` : clôt les précédentes avec leur durée.
    fn enter_step(&mut self, step: usize, now: Instant) {
        for previous in 0..step {
            if self.step_took[previous].is_none() {
                let started = self.step_started[previous].unwrap_or(now);
                self.step_took[previous] = Some(now - started);
                if self.step_started[previous + 1].is_none() {
                    self.step_started[previous + 1] = Some(now);
                }
            }
        }
        if self.step_started[step].is_none() {
            self.step_started[step] = Some(now);
        }
    }

    /// Fait avancer l'étape, la jauge et le fichier affiché.
    fn advance(&mut self) {
        let now = Instant::now();
        let (phase, files, rules, done) = match self.phase.lock() {
            Ok(progress) => (
                progress.phase.clone(),
                if self.files.is_empty() {
                    progress.files.clone()
                } else {
                    Vec::new()
                },
                progress.rules,
                progress.done,
            ),
            Err(_) => return,
        };
        self.rules = rules;
        self.enter_step(Self::step_of(&phase), now);
        if self.files.is_empty() && !files.is_empty() {
            self.files = files.into_iter().map(Into::into).collect();
        }

        // Avant l'analyse, la jauge avance par étape ; pendant, elle suit l'estimation.
        let percent = match phase {
            Phase::Preparing => 2,
            Phase::Rules => 5,
            Phase::Reading { .. } => 8,
            Phase::Analyzing { files } => {
                let started = self.step_started[3].unwrap_or(now);
                let elapsed = (now - started).as_secs_f32() * 1000.;
                let estimate =
                    (2_000. + files as f32 * 100.).clamp(MIN_ESTIMATE_MS, MAX_ESTIMATE_MS);
                // Fichiers réellement terminés (rapportés par le moteur) ;
                // avant le premier, l'estimation au temps.
                let ratio = if done > 0 && files > 0 {
                    (done as f32 / files as f32).min(1.)
                } else {
                    (elapsed / estimate).min(1.)
                };
                if !self.files.is_empty() {
                    let position =
                        ((ratio * self.files.len() as f32) as usize).min(self.files.len() - 1);
                    self.current = Some(position);
                }
                let eased = 1. - (1. - ratio) * (1. - ratio);
                (8. + (PROGRESS_CAP - 8.) * eased) as u32
            }
            Phase::Dependencies => PROGRESS_CAP as u32 + 2,
            Phase::Scoring => PROGRESS_CAP as u32 + 5,
        };
        self.percent = self.percent.max(percent);
    }

    fn phase_label(&self) -> String {
        match (self.current, self.phase.lock().map(|p| p.phase.clone())) {
            (Some(position), _) => format!(
                "Analyse en cours · {} / {} fichiers",
                position + 1,
                self.files.len()
            ),
            (_, Ok(Phase::Rules)) => match self.rules {
                Some(n) if n > 0 => format!("{n} règles chargées"),
                _ => "Chargement des règles…".to_owned(),
            },
            (_, Ok(phase)) => phase.label(),
            _ => String::new(),
        }
    }
}

fn close_button(theme: Theme, cx: &mut Context<HeraldApp>) -> impl IntoElement {
    div()
        .id("analysis-dismiss")
        .flex_none()
        .p(metrics::SPACE_1)
        .rounded(metrics::RADIUS)
        .cursor_pointer()
        .hover(move |style| style.bg(theme.bg_hover))
        .on_click(cx.listener(|app, _, _, cx| app.dismiss_analysis(cx)))
        .child(Icon::new(IconName::Close, theme.ink_soft).size(metrics::ICON_SM))
}

fn grade_tone(score: u8, theme: &Theme) -> Hsla {
    if score >= 80 {
        theme.success
    } else if score >= 60 {
        theme.warning
    } else {
        theme.danger
    }
}

/// Jauge circulaire : la piste complète, puis l'arc de `value` (0 à 1) depuis le haut.
fn ring(value: f32, color: Hsla, track: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let center = bounds.center();
            let radius = bounds.size.width.min(bounds.size.height) / 2. - GAUGE_STROKE / 2.;
            let at = |turn: f32| {
                let angle = (turn - 0.25) * std::f32::consts::TAU;
                point(
                    center.x + radius * angle.cos(),
                    center.y + radius * angle.sin(),
                )
            };
            let arc = |from: f32, to: f32| {
                let mut path = PathBuilder::stroke(GAUGE_STROKE);
                path.move_to(at(from));
                path.arc_to(point(radius, radius), px(0.), to - from > 0.5, true, at(to));
                path.build().ok()
            };
            // Un arc ne peut pas faire un tour complet : deux moitiés.
            for (from, to) in [(0., 0.5), (0.5, 0.9999)] {
                if let Some(path) = arc(from, to) {
                    window.paint_path(path, track);
                }
            }
            let value = value.clamp(0., 0.9999);
            if value > 0.005 {
                if let Some(path) = arc(0., value.min(0.5)) {
                    window.paint_path(path, color);
                }
                if value > 0.5 {
                    if let Some(path) = arc(0.5, value) {
                        window.paint_path(path, color);
                    }
                }
            }
        },
    )
    .size_full()
}

/// Durée en secondes, à la française (« 2,4 s »).
fn seconds(duration: Duration) -> String {
    format!("{:.1} s", duration.as_secs_f32()).replace('.', ",")
}

/// Ce qu'on lit de l'avancement partagé avec le moteur.
struct Live {
    done: usize,
    last_file: Option<String>,
    by_severity: [usize; 3],
}

/// Avancement lisible d'une analyse en cours (carte ou bandeau du héraut).
pub(super) struct RunProgress {
    pub(super) title: String,
    /// Temps écoulé, problèmes et critiques trouvés.
    pub(super) facts: String,
    /// Ce que fait le moteur, ou le dernier fichier lu.
    pub(super) detail: SharedString,
    pub(super) percent: u32,
}

impl HeraldApp {
    /// Avancement de l'analyse en cours du projet affiché.
    pub(super) fn run_progress(&self) -> Option<RunProgress> {
        let run = self.analysis_run.as_ref()?;
        let project = self.project()?;
        (run.path == project.path && !run.is_over()).then(|| run.progress())
    }

    /// L'analyse du projet affiché a échoué (sa carte reste à montrer).
    pub(super) fn run_failed(&self) -> bool {
        self.analysis_run.as_ref().is_some_and(|run| {
            run.failure.is_some() && self.project().is_some_and(|p| p.path == run.path)
        })
    }
}

impl AnalysisRun {
    fn progress(&self) -> RunProgress {
        let live = self.live();
        let elapsed = self.step_started[0]
            .map(|start| Instant::now() - start)
            .unwrap_or_default();
        let total = self.files.len();
        let analyzing_files = self.step_started[3].is_some() && self.step_took[3].is_none();
        let title = if analyzing_files && total > 0 {
            format!(
                "Analyse en cours · {} / {total} fichiers",
                live.done.min(total)
            )
        } else {
            self.phase_label()
        };
        let found: usize = live.by_severity.iter().sum();
        let mut facts = vec![seconds(elapsed)];
        if found > 0 {
            facts.push(count_label(
                found as u32,
                "problème trouvé",
                "problèmes trouvés",
            ));
            if live.by_severity[0] > 0 {
                facts.push(count_label(
                    live.by_severity[0] as u32,
                    "critique",
                    "critiques",
                ));
            }
        }
        let detail: SharedString = match (&live.last_file, analyzing_files, live.done) {
            (_, true, 0) => "Compilation des règles…".into(),
            (_, true, done) if done >= total => "Analyse croisée entre les fichiers…".into(),
            (Some(file), true, _) => file.clone().into(),
            _ => "Analyse locale · aucun fichier n'est envoyé".into(),
        };
        RunProgress {
            title,
            facts: facts.join(" · "),
            detail,
            percent: self.percent.min(100),
        }
    }

    fn live(&self) -> Live {
        match self.phase.lock() {
            Ok(progress) => Live {
                done: progress.done,
                last_file: progress.last_file.clone(),
                by_severity: progress.by_severity,
            },
            Err(_) => Live {
                done: 0,
                last_file: None,
                by_severity: [0; 3],
            },
        }
    }
}

/// Petite icône qui tourne (aussi utilisée par la barre latérale).
pub(super) fn spinner(theme: Theme) -> impl IntoElement {
    svg()
        .path("icons/refresh.svg")
        .size(SPINNER_SIZE)
        .text_color(theme.accent)
        .with_animation(
            "analysis-spinner",
            Animation::new(Duration::from_millis(800)).repeat(),
            |icon, delta| {
                icon.with_transformation(Transformation::rotate(radians(
                    delta * std::f32::consts::TAU,
                )))
            },
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_follow_engine_phases() {
        assert_eq!(AnalysisRun::step_of(&Phase::Preparing), 0);
        assert_eq!(AnalysisRun::step_of(&Phase::Rules), 1);
        assert_eq!(AnalysisRun::step_of(&Phase::Analyzing { files: 3 }), 3);
        assert_eq!(AnalysisRun::step_of(&Phase::Scoring), STEPS - 1);
    }

    #[test]
    fn step_durations_use_a_decimal_comma() {
        assert_eq!(seconds(Duration::from_millis(2_400)), "2,4 s");
        assert_eq!(seconds(Duration::ZERO), "0,0 s");
    }
}

/// Échec d'une analyse.
pub(super) enum RunError {
    /// Quota du jour atteint (refus du serveur).
    #[allow(dead_code)]
    Quota(String),
    Analysis(String),
}

//! Ressources consommées par Herald pendant une analyse : une barre d'état en bas
//! de la fenêtre affiche la mémoire vive du processus, une courbe de la mémoire
//! depuis le lancement, le pic, la part due à l'analyse et le processeur. À la
//! fin, elle garde le bilan (courbe, pic, durée) jusqu'à la fermeture de la carte.
//!
//! Le moteur tourne dans le processus de l'app : la mémoire mesurée est donc bien
//! celle de l'analyse, plus celle de l'interface (mesurée au départ, retirée pour
//! « Analyse »). Les mesures sont prises hors du fil de l'interface toutes les
//! `SAMPLE_EVERY` et s'arrêtent avec l'analyse.

use std::time::{Duration, Instant};

use gpui::{FontWeight, PathBuilder, Pixels, Task, canvas, div, point, prelude::*, px};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use super::app::HeraldApp;
use crate::theme::{Theme, fonts, metrics};

const SAMPLE_EVERY: Duration = Duration::from_millis(500);
/// Points de la courbe au plus : au-delà, deux points voisins sont fusionnés
/// (on garde le plus haut) pour que la courbe couvre toujours toute l'analyse.
const MAX_POINTS: usize = 120;
/// Écart minimal représenté par la hauteur de la courbe, pour qu'une mémoire
/// stable ne dessine pas un bruit de quelques kilo-octets en pleine hauteur.
const MIN_RANGE: u64 = 16 * 1024 * 1024;

const SPARK_W: Pixels = px(160.);
const SPARK_H: Pixels = px(16.);
const SPARK_STROKE: Pixels = px(1.5);
const SPARK_BASE: Pixels = px(1.);

/// Une mesure du processus Herald.
#[derive(Clone, Copy)]
struct Sample {
    /// Mémoire résidente, en octets.
    memory: u64,
    /// Processeur, en pourcentage de la machine entière (0 à 100).
    cpu: f32,
}

/// Lit la mémoire et le processeur du processus courant.
struct Sampler {
    system: System,
    pid: Option<Pid>,
    cores: f32,
}

impl Sampler {
    fn new() -> Self {
        Self {
            system: System::new(),
            pid: sysinfo::get_current_pid().ok(),
            cores: std::thread::available_parallelism().map_or(1., |n| n.get() as f32),
        }
    }

    fn sample(&mut self) -> Option<Sample> {
        let pid = self.pid?;
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            ProcessRefreshKind::new().with_memory().with_cpu(),
        );
        let process = self.system.process(pid)?;
        Some(Sample {
            memory: process.memory(),
            cpu: (process.cpu_usage() / self.cores).clamp(0., 100.),
        })
    }
}

/// Mesures d'une analyse.
pub(super) struct ResourceTrace {
    /// Mémoire au lancement de l'analyse (interface seule).
    baseline: Option<u64>,
    /// Points de la courbe ; chacun couvre `per_point` mesures (leur maximum).
    points: Vec<u64>,
    per_point: usize,
    /// Maximum des mesures pas encore rangées dans `points`, et leur nombre.
    pending: (u64, usize),
    current: Option<Sample>,
    peak: u64,
    started: Instant,
    finished: Option<Instant>,
}

impl ResourceTrace {
    pub(super) fn new() -> Self {
        Self {
            baseline: None,
            points: Vec::with_capacity(MAX_POINTS),
            per_point: 1,
            pending: (0, 0),
            current: None,
            peak: 0,
            started: Instant::now(),
            finished: None,
        }
    }

    fn record(&mut self, sample: Sample) {
        self.baseline.get_or_insert(sample.memory);
        self.peak = self.peak.max(sample.memory);
        self.current = Some(sample);

        let (max, count) = &mut self.pending;
        *max = (*max).max(sample.memory);
        *count += 1;
        if *count < self.per_point {
            return;
        }
        self.points.push(*max);
        self.pending = (0, 0);
        if self.points.len() == MAX_POINTS {
            self.points = self
                .points
                .chunks(2)
                .map(|pair| pair.iter().copied().max().unwrap_or_default())
                .collect();
            self.per_point *= 2;
        }
    }

    /// Courbe à dessiner, mesures en attente comprises.
    fn curve(&self) -> Vec<u64> {
        let mut curve = self.points.clone();
        if self.pending.1 > 0 {
            curve.push(self.pending.0);
        }
        curve
    }

    /// Mémoire ajoutée par l'analyse au moment du pic.
    fn analysis_peak(&self) -> u64 {
        self.baseline
            .map_or(0, |baseline| self.peak.saturating_sub(baseline))
    }

    /// Fige la durée à la fin de l'analyse.
    pub(super) fn finish(&mut self) {
        self.finished.get_or_insert_with(Instant::now);
    }

    fn elapsed(&self) -> Duration {
        self.finished.unwrap_or_else(Instant::now) - self.started
    }
}

/// Mesure les ressources tant que l'analyse en cours n'est pas terminée.
pub(super) fn spawn_monitor(cx: &mut Context<HeraldApp>) -> Task<()> {
    cx.spawn(async move |this, cx| {
        let mut sampler = Sampler::new();
        loop {
            // La lecture du système se fait hors du fil de l'interface.
            let (returned, sample) = cx
                .background_executor()
                .spawn(async move {
                    let sample = sampler.sample();
                    (sampler, sample)
                })
                .await;
            sampler = returned;
            let over = this
                .update(cx, |app, cx| {
                    let Some(run) = &mut app.analysis_run else {
                        return true;
                    };
                    if let Some(sample) = sample {
                        run.resources.record(sample);
                    }
                    cx.notify();
                    run.is_over()
                })
                .unwrap_or(true);
            if over {
                break;
            }
            cx.background_executor().timer(SAMPLE_EVERY).await;
        }
    })
}

impl HeraldApp {
    /// Barre d'état en bas de la fenêtre, tant qu'une analyse est affichée.
    pub(super) fn render_resource_bar(&self, theme: Theme) -> Option<gpui::AnyElement> {
        let run = self.analysis_run.as_ref()?;
        let trace = &run.resources;
        let current = trace.current?;
        let over = run.is_over();

        let label = |text: &'static str| div().text_color(theme.ink_soft).child(text);
        let value = |text: String| {
            div()
                .font_family(fonts::MONO)
                .text_color(theme.ink_muted)
                .child(text)
        };

        let mut bar = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(metrics::SPACE_3)
            .px(metrics::SPACE_4)
            .py(metrics::SPACE_2)
            .border_t_1()
            .border_color(theme.border_soft)
            .bg(theme.bg_surface)
            .text_size(metrics::TEXT_SM)
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.ink_muted)
                    .child(if over {
                        "Ressources de l'analyse"
                    } else {
                        "Ressources de Herald"
                    }),
            );
        if !over {
            bar = bar
                .child(label("Mémoire"))
                .child(value(format_bytes(current.memory)));
        }
        bar = bar
            .child(sparkline(trace.curve(), theme))
            .child(label(if over { "Pic mémoire" } else { "Pic" }))
            .child(value(format_bytes(trace.peak)))
            .child(label("Analyse"))
            .child(value(format!("+{}", format_bytes(trace.analysis_peak()))));
        bar = if over {
            bar.child(label("Durée"))
                .child(value(format_duration(trace.elapsed())))
        } else {
            bar.child(label("Processeur"))
                .child(value(format!("{:.0} %", current.cpu)))
        };
        Some(bar.into_any_element())
    }
}

/// Courbe de la mémoire sur toute l'analyse, du minimum (bas) au pic (haut).
fn sparkline(points: Vec<u64>, theme: Theme) -> impl IntoElement {
    let (track, line) = (theme.border_soft, theme.accent);
    div().flex_none().w(SPARK_W).h(SPARK_H).child(
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let mut base = PathBuilder::stroke(SPARK_BASE);
                base.move_to(point(bounds.left(), bounds.bottom()));
                base.line_to(point(bounds.right(), bounds.bottom()));
                if let Ok(path) = base.build() {
                    window.paint_path(path, track);
                }
                if points.len() < 2 {
                    return;
                }
                let low = points.iter().copied().min().unwrap_or_default();
                let high = points.iter().copied().max().unwrap_or_default();
                let range = (high - low).max(MIN_RANGE) as f32;
                let bottom = bounds.bottom() - SPARK_STROKE;
                let height = bounds.size.height - SPARK_STROKE * 2.;
                let step = bounds.size.width / (points.len() - 1) as f32;
                let mut path = PathBuilder::stroke(SPARK_STROKE);
                for (ix, memory) in points.iter().enumerate() {
                    let at = point(
                        bounds.left() + step * ix as f32,
                        bottom - height * ((memory - low) as f32 / range),
                    );
                    if ix == 0 {
                        path.move_to(at);
                    } else {
                        path.line_to(at);
                    }
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, line);
                }
            },
        )
        .size_full(),
    )
}

/// « 312 Mo », « 1,2 Go » (unités binaires, comme le Moniteur d'activité).
fn format_bytes(bytes: u64) -> String {
    const MB: f64 = 1024. * 1024.;
    let mb = bytes as f64 / MB;
    if mb >= 1024. {
        format!("{:.1} Go", mb / 1024.).replace('.', ",")
    } else {
        format!("{mb:.0} Mo")
    }
}

/// « 8 s », « 2 min 05 s ».
fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{secs} s")
    } else {
        format!("{} min {:02} s", secs / 60, secs % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record_all(trace: &mut ResourceTrace, memories: impl IntoIterator<Item = u64>) {
        for memory in memories {
            trace.record(Sample { memory, cpu: 0. });
        }
    }

    #[test]
    fn bytes_are_readable() {
        assert_eq!(format_bytes(312 * 1024 * 1024), "312 Mo");
        assert_eq!(format_bytes(1229 * 1024 * 1024), "1,2 Go");
    }

    #[test]
    fn durations_are_readable() {
        assert_eq!(format_duration(Duration::from_secs(8)), "8 s");
        assert_eq!(format_duration(Duration::from_secs(125)), "2 min 05 s");
    }

    #[test]
    fn peak_and_analysis_share() {
        let mut trace = ResourceTrace::new();
        record_all(&mut trace, [100, 180, 140]);
        assert_eq!(trace.peak, 180);
        assert_eq!(trace.analysis_peak(), 80);
        assert_eq!(trace.curve(), vec![100, 180, 140]);
    }

    #[test]
    fn long_analyses_are_compacted_without_losing_the_peak() {
        let mut trace = ResourceTrace::new();
        let total = MAX_POINTS as u64 * 3 + 1;
        record_all(&mut trace, 0..total);
        let curve = trace.curve();
        assert!(curve.len() <= MAX_POINTS);
        assert_eq!(curve.last(), Some(&(total - 1)));
        assert_eq!(trace.peak, total - 1);
    }

    #[test]
    fn sampler_reads_own_process() {
        let sample = Sampler::new().sample().expect("mesure du processus");
        assert!(sample.memory > 0);
    }
}

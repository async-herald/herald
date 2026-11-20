//! Onglet « Dépendances » d'un projet : chaque paquet (version installée lue
//! dans les lockfiles, sinon version déclarée) et ses vulnérabilités connues
//! dans la base OSV.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use gpui::{AnyElement, FontWeight, Hsla, SharedString, div, prelude::*, px};

use super::analysis::severity_color;
use super::app::HeraldApp;
use super::components::{Button, ButtonVariant, SeverityLabel};
use super::home::{count_label, list_row, section_header, surface};
use crate::engine::cve::{self, Dependency};
use crate::model::{Advisory, Severity};
use crate::store;
use crate::theme::{Theme, fonts, metrics};

/// Paquets affichés par page.
const PAGE_SIZE: usize = 50;
const EXPLANATION_MAX_WIDTH: gpui::Pixels = px(640.);
const VERSION_W: gpui::Pixels = px(120.);
const KIND_W: gpui::Pixels = px(80.);
const ECOSYSTEM_W: gpui::Pixels = px(80.);
const STATUS_W: gpui::Pixels = px(150.);
const FIX_W: gpui::Pixels = px(110.);
/// Retrait des avis sous le paquet déplié.
const ADVISORY_INDENT: gpui::Pixels = px(32.);

/// Paquets montrés.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DependencyFilter {
    Vulnerable,
    Direct,
    All,
}

/// État de l'onglet, commun à tous les projets.
#[derive(Default)]
pub(super) struct DependencyTab {
    /// `None` : « Vulnérables » s'il y en a, sinon « Toutes ».
    filter: Option<DependencyFilter>,
    page: usize,
    /// Paquet dont les avis sont dépliés (`écosystème/nom@version`).
    expanded: Option<SharedString>,
    /// Revérification OSV en cours pour ce projet.
    checking: Option<PathBuf>,
    /// Échec de la dernière revérification.
    check_error: Option<SharedString>,
}

/// Un paquet et ce qu'OSV en dit.
struct Row<'a> {
    dep: &'a Dependency,
    advisories: Vec<&'a Advisory>,
    /// Gravité la plus haute de ses avis.
    severity: Option<Severity>,
    /// Version qui corrige tous ses avis.
    fix: Option<&'a str>,
}

impl Row<'_> {
    fn key(&self) -> SharedString {
        format!(
            "{}/{}@{}",
            self.dep.ecosystem, self.dep.name, self.dep.version
        )
        .into()
    }

    fn is_vulnerable(&self) -> bool {
        !self.advisories.is_empty()
    }
}

impl HeraldApp {
    fn set_dependency_filter(&mut self, filter: DependencyFilter, cx: &mut Context<Self>) {
        self.dependency_tab.filter = Some(filter);
        self.dependency_tab.page = 0;
        cx.notify();
    }

    fn set_dependency_page(&mut self, page: usize, cx: &mut Context<Self>) {
        self.dependency_tab.page = page;
        cx.notify();
    }

    fn toggle_dependency(&mut self, key: SharedString, cx: &mut Context<Self>) {
        let tab = &mut self.dependency_tab;
        tab.expanded = if tab.expanded.as_ref() == Some(&key) {
            None
        } else {
            Some(key)
        };
        cx.notify();
    }

    /// Revérifie auprès d'OSV les dépendances du projet affiché, sans relancer
    /// l'analyse (ni décompter de quota).
    fn recheck_dependencies(&mut self, cx: &mut Context<Self>) {
        let Some(project) = self.project() else {
            return;
        };
        if self.dependency_tab.checking.is_some() {
            return;
        }
        let path = project.path.clone();
        let name = project.name.to_string();
        let deps = self.dependencies.get(&path).cloned().unwrap_or_default();
        self.dependency_tab.checking = Some(path.clone());
        self.dependency_tab.check_error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let scanned = cx
                .background_executor()
                .spawn(async move {
                    let scanned = cve::scan(&name, &path, &deps);
                    if let Ok(advisories) = &scanned {
                        store::replace_alerts(&path, advisories);
                    }
                    scanned.map(|_| ())
                })
                .await;
            this.update(cx, |app, cx| {
                app.dependency_tab.checking = None;
                match scanned {
                    Ok(()) => app.alerts = store::load_alerts(),
                    Err(err) => app.dependency_tab.check_error = Some(err.into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn render_dependencies(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(project) = self.project() else {
            return div().into_any_element();
        };
        let deps: &[Dependency] = self
            .dependencies
            .get(&project.path)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let rows = rows(deps, &self.alerts, &project.path);

        let vulnerable = rows.iter().filter(|row| row.is_vulnerable()).count();
        let direct = rows.iter().filter(|row| row.dep.direct).count();
        let filter = self.dependency_tab.filter.unwrap_or(if vulnerable > 0 {
            DependencyFilter::Vulnerable
        } else {
            DependencyFilter::All
        });
        let shown: Vec<&Row> = rows
            .iter()
            .filter(|row| match filter {
                DependencyFilter::Vulnerable => row.is_vulnerable(),
                DependencyFilter::Direct => row.dep.direct,
                DependencyFilter::All => true,
            })
            .collect();

        let total = shown.len();
        let pages = total.div_ceil(PAGE_SIZE).max(1);
        let page = self.dependency_tab.page.min(pages - 1);
        let first = page * PAGE_SIZE;
        let mut list: Vec<AnyElement> = Vec::new();
        for (offset, row) in shown.iter().skip(first).take(PAGE_SIZE).enumerate() {
            let ix = first + offset;
            let key = row.key();
            let expanded = self.dependency_tab.expanded.as_ref() == Some(&key);
            list.push(dependency_row(ix, row, key, expanded, theme, cx).into_any_element());
            if expanded {
                list.extend(row.advisories.iter().enumerate().map(|(n, advisory)| {
                    advisory_row((ix, n), advisory, theme).into_any_element()
                }));
                if !row.dep.resolved {
                    list.push(unresolved_note(row.dep, theme).into_any_element());
                }
            }
        }

        let checking = self.dependency_tab.checking.as_ref() == Some(&project.path);
        let detail = format!(
            "{} · {} · {} au total",
            count_label(vulnerable as u32, "vulnérable", "vulnérables"),
            count_label(direct as u32, "directe", "directes"),
            rows.len()
        );
        let recheck = if checking {
            div()
                .opacity(0.6)
                .child(
                    Button::new("recheck-deps", "Vérification…", theme)
                        .variant(ButtonVariant::Ghost),
                )
                .into_any_element()
        } else {
            Button::new("recheck-deps", "Revérifier auprès d'OSV", theme)
                .variant(ButtonVariant::Ghost)
                .on_click(cx.listener(|app, _, _, cx| app.recheck_dependencies(cx)))
                .into_any_element()
        };

        let empty_message = match (deps.is_empty(), filter) {
            (true, _) => Some(
                "Aucune dépendance trouvée. Herald lit package.json, Cargo.toml, requirements.txt, \
                 go.mod et leurs lockfiles.",
            ),
            (false, DependencyFilter::Vulnerable) if total == 0 => {
                Some("Aucune vulnérabilité connue dans ces dépendances.")
            }
            _ => None,
        };

        let pagination = (pages > 1).then(|| {
            list_row(theme)
                .items_center()
                .text_size(metrics::TEXT_SM)
                .text_color(theme.ink_muted)
                .child(div().flex_1().child(format!(
                    "{}–{} sur {total} · page {} sur {pages}",
                    first + 1,
                    (first + PAGE_SIZE).min(total),
                    page + 1
                )))
                .child(page_button(
                    "deps-prev",
                    "Précédent",
                    page.checked_sub(1),
                    theme,
                    cx,
                ))
                .child(page_button(
                    "deps-next",
                    "Suivant",
                    (page + 1 < pages).then_some(page + 1),
                    theme,
                    cx,
                ))
        });

        div()
            .flex()
            .flex_col()
            .gap(metrics::SPACE_5)
            .child(
                div()
                    .max_w(EXPLANATION_MAX_WIDTH)
                    .flex()
                    .flex_col()
                    .gap(metrics::SPACE_1)
                    .text_color(theme.ink_muted)
                    .child(source_sentence(deps))
                    .children(
                        self.dependency_tab
                            .check_error
                            .clone()
                            .filter(|_| !checking)
                            .map(|err| div().text_color(theme.danger).child(err)),
                    ),
            )
            .child(
                surface(theme)
                    .flex()
                    .flex_col()
                    .child(
                        section_header("Paquets", detail, theme)
                            .child(recheck)
                            .child(filter_switch(filter, theme, cx)),
                    )
                    .when(!deps.is_empty() && total > 0, |this| {
                        this.child(column_headers(theme))
                    })
                    .children(list)
                    .children(
                        empty_message.map(|message| {
                            list_row(theme).text_color(theme.ink_muted).child(message)
                        }),
                    )
                    .children(pagination),
            )
            .into_any_element()
    }
}

/// Paquets du projet avec leurs avis : vulnérables d'abord (les plus graves en
/// tête), puis directs, puis par nom.
fn rows<'a>(deps: &'a [Dependency], alerts: &'a [Advisory], project: &PathBuf) -> Vec<Row<'a>> {
    let mut by_package: HashMap<(&str, &str, &str), Vec<&Advisory>> = HashMap::new();
    for alert in alerts.iter().filter(|alert| &alert.project_path == project) {
        by_package
            .entry((
                alert.ecosystem.as_ref(),
                alert.package.as_ref(),
                alert.installed_version.as_ref(),
            ))
            .or_default()
            .push(alert);
    }
    let mut rows: Vec<Row> = deps
        .iter()
        .map(|dep| {
            let mut advisories = by_package
                .remove(&(
                    dep.ecosystem.as_str(),
                    dep.name.as_str(),
                    dep.version.as_str(),
                ))
                .unwrap_or_default();
            advisories.sort_by(|a, b| {
                a.severity
                    .cmp(&b.severity)
                    .then(b.cvss.unwrap_or(0.).total_cmp(&a.cvss.unwrap_or(0.)))
            });
            let severity = advisories.iter().map(|a| a.severity).min();
            let fix = cve::highest_version(
                advisories
                    .iter()
                    .filter_map(|a| a.fixed_version.as_ref().map(|v| v.as_ref())),
            );
            Row {
                dep,
                advisories,
                severity,
                fix,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        let rank = |row: &Row| row.severity.map_or(u8::MAX, |s| s as u8);
        rank(a)
            .cmp(&rank(b))
            .then(b.dep.direct.cmp(&a.dep.direct))
            .then_with(|| a.dep.name.to_lowercase().cmp(&b.dep.name.to_lowercase()))
    });
    rows
}

/// D'où viennent les versions : lockfiles (installées) ou manifestes seuls.
fn source_sentence(deps: &[Dependency]) -> String {
    let lockfiles: BTreeSet<&str> = deps
        .iter()
        .filter(|dep| dep.resolved && crate::engine::lockfile::is_lockfile(&dep.source))
        .map(|dep| file_name(&dep.source))
        .collect();
    let unresolved = deps.iter().filter(|dep| !dep.resolved).count();
    let mut sentence = match lockfiles.len() {
        0 => "Versions déclarées dans les manifestes, aucun lockfile trouvé.".to_owned(),
        _ => format!(
            "Versions installées, lues dans {}.",
            lockfiles.into_iter().collect::<Vec<_>>().join(", ")
        ),
    };
    if unresolved > 0 {
        sentence.push_str(&format!(
            " {} sans version exacte : une vulnérabilité y est à confirmer, la version \
             installée pouvant déjà être corrigée.",
            count_label(unresolved as u32, "dépendance", "dépendances")
        ));
    }
    sentence
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn column_headers(theme: Theme) -> impl IntoElement {
    list_row(theme)
        .py(metrics::SPACE_2)
        .text_size(metrics::TEXT_SM)
        .text_color(theme.ink_soft)
        .child(div().flex_1().min_w_0().child("Paquet"))
        .child(div().w(VERSION_W).flex_none().child("Version"))
        .child(div().w(KIND_W).flex_none().child("Type"))
        .child(div().w(ECOSYSTEM_W).flex_none().child("Écosystème"))
        .child(div().w(STATUS_W).flex_none().child("Vulnérabilités"))
        .child(
            div()
                .w(FIX_W)
                .flex_none()
                .flex()
                .justify_end()
                .child("Corrigé en"),
        )
}

fn dependency_row(
    ix: usize,
    row: &Row,
    key: SharedString,
    expanded: bool,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> impl IntoElement + use<> {
    let dep = row.dep;
    let version = if dep.resolved {
        dep.version.clone()
    } else {
        format!("≥ {}", dep.version)
    };
    let status_color: Hsla = match (row.severity, dep.resolved) {
        (Some(_), false) => theme.ink_muted,
        (Some(severity), true) => severity_color(severity, &theme),
        (None, _) => theme.ink_soft,
    };
    let status = match (row.advisories.len(), dep.resolved) {
        (0, _) => "Aucune connue".to_owned(),
        (n, true) => count_label(n as u32, "vulnérabilité", "vulnérabilités"),
        (n, false) => format!("{n} à confirmer"),
    };
    let fix = match (row.is_vulnerable(), row.fix) {
        (false, _) => String::new(),
        (true, Some(fix)) => fix.to_owned(),
        (true, None) => "aucune".to_owned(),
    };
    let vulnerable = row.is_vulnerable();

    list_row(theme)
        .id(("dependency", ix))
        .items_center()
        .when(expanded, |this| this.bg(theme.bg_active))
        .when(vulnerable, |this| {
            this.cursor_pointer()
                .hover(|style| style.bg(theme.bg_hover))
                .on_click(cx.listener(move |app, _, _, cx| app.toggle_dependency(key.clone(), cx)))
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_family(fonts::MONO)
                .font_weight(FontWeight::MEDIUM)
                .child(dep.name.clone()),
        )
        .child(
            div()
                .w(VERSION_W)
                .flex_none()
                .truncate()
                .font_family(fonts::MONO)
                .text_size(metrics::TEXT_SM)
                .when(!dep.resolved, |this| this.text_color(theme.ink_muted))
                .child(version),
        )
        .child(
            div()
                .w(KIND_W)
                .flex_none()
                .text_size(metrics::TEXT_SM)
                .text_color(theme.ink_muted)
                .child(if dep.direct { "directe" } else { "transitive" }),
        )
        .child(
            div()
                .w(ECOSYSTEM_W)
                .flex_none()
                .truncate()
                .text_size(metrics::TEXT_SM)
                .text_color(theme.ink_muted)
                .child(dep.ecosystem.clone()),
        )
        .child(
            div()
                .w(STATUS_W)
                .flex_none()
                .flex()
                .items_center()
                .gap(metrics::SPACE_2)
                .text_size(metrics::TEXT_SM)
                .text_color(status_color)
                .when(vulnerable, |this| {
                    this.font_weight(FontWeight::MEDIUM)
                        .child(div().size(metrics::DOT).rounded_full().bg(status_color))
                })
                .child(status),
        )
        .child(
            div()
                .w(FIX_W)
                .flex_none()
                .flex()
                .justify_end()
                .font_family(fonts::MONO)
                .text_size(metrics::TEXT_SM)
                .text_color(if row.fix.is_some() {
                    theme.accent_text
                } else {
                    theme.ink_muted
                })
                .child(fix),
        )
}

/// Un avis OSV sous le paquet déplié.
fn advisory_row(id: (usize, usize), advisory: &Advisory, theme: Theme) -> impl IntoElement + use<> {
    let cvss = advisory
        .cvss
        .map(|score| format!("CVSS {score:.1}").replace('.', ","));
    let published = cve::published_label(advisory.published_unix, cve::unix_now());
    let url = advisory.url.clone();
    let fixed = advisory.fixed_version.as_ref().map_or_else(
        || "pas encore de correctif".to_owned(),
        |v| format!("corrigé en {v}"),
    );

    list_row(theme).pl(ADVISORY_INDENT).bg(theme.bg_page).child(
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(metrics::SPACE_1)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(metrics::SPACE_3)
                    .text_size(metrics::TEXT_SM)
                    .text_color(theme.ink_muted)
                    .child(SeverityLabel::new(advisory.severity, theme))
                    .child(div().font_family(fonts::MONO).child(advisory.id.clone()))
                    .children(cvss)
                    .child(fixed)
                    .child(div().flex_1())
                    .child(format!("publiée {published}"))
                    .child(
                        Button::new(("dep-advisory", id.0 * 1_000 + id.1), "Voir l'avis", theme)
                            .variant(ButtonVariant::Ghost)
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    ),
            )
            .child(div().child(advisory.summary.clone())),
    )
}

/// Sous un paquet sans version exacte : pourquoi ses avis sont à confirmer.
fn unresolved_note(dep: &Dependency, theme: Theme) -> impl IntoElement + use<> {
    list_row(theme)
        .pl(ADVISORY_INDENT)
        .bg(theme.bg_page)
        .text_size(metrics::TEXT_SM)
        .text_color(theme.ink_muted)
        .child(format!(
            "{} déclare {} {} au minimum, sans lockfile : la version installée peut être plus \
             récente et déjà corrigée. Ajoutez le lockfile au dépôt pour un résultat exact.",
            dep.source, dep.name, dep.version
        ))
}

/// Vulnérables / Directes / Toutes.
fn filter_switch(
    current: DependencyFilter,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> impl IntoElement {
    let item = |id: &'static str, label: &'static str, filter: DependencyFilter| {
        let active = current == filter;
        div()
            .id(id)
            .flex()
            .items_center()
            .h_full()
            .px(metrics::SPACE_3)
            .cursor_pointer()
            .text_size(metrics::TEXT_SM)
            .when(active, |this| {
                this.bg(theme.bg_active).font_weight(FontWeight::MEDIUM)
            })
            .when(!active, |this| {
                this.text_color(theme.ink_muted)
                    .hover(|style| style.bg(theme.bg_hover))
            })
            .on_click(cx.listener(move |app, _, _, cx| app.set_dependency_filter(filter, cx)))
            .child(label)
    };
    div()
        .flex()
        .flex_none()
        .h(metrics::CONTROL_H)
        .rounded(metrics::RADIUS)
        .overflow_hidden()
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg_surface)
        .child(item(
            "deps-vulnerable",
            "Vulnérables",
            DependencyFilter::Vulnerable,
        ))
        .child(div().w(px(1.)).h_full().bg(theme.border))
        .child(item("deps-direct", "Directes", DependencyFilter::Direct))
        .child(div().w(px(1.)).h_full().bg(theme.border))
        .child(item("deps-all", "Toutes", DependencyFilter::All))
}

/// Bouton de pagination ; estompé et inactif sans page cible.
fn page_button(
    id: &'static str,
    label: &'static str,
    target: Option<usize>,
    theme: Theme,
    cx: &mut Context<HeraldApp>,
) -> impl IntoElement {
    let button = Button::new(id, label, theme).variant(ButtonVariant::Ghost);
    match target {
        Some(page) => div().child(
            button.on_click(cx.listener(move |app, _, _, cx| app.set_dependency_page(page, cx))),
        ),
        None => div().opacity(0.4).child(button),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::advisory_for_tests;

    fn dep(name: &str, version: &str, direct: bool) -> Dependency {
        Dependency {
            ecosystem: "npm".into(),
            name: name.into(),
            version: version.into(),
            direct,
            resolved: true,
            source: "package-lock.json".into(),
        }
    }

    #[test]
    fn vulnerable_packages_come_first_with_the_highest_fix() {
        let deps = vec![
            dep("a-transitive", "1.0.0", false),
            dep("b-direct", "1.0.0", true),
            dep("express", "4.18.2", true),
        ];
        let mut first = advisory_for_tests(Some("4.19.2"));
        first.severity = Severity::Warning;
        let second = advisory_for_tests(Some("4.21.1"));
        let alerts = vec![first, second];
        let project = PathBuf::from("/tmp/demo");

        let rows = rows(&deps, &alerts, &project);
        let names: Vec<&str> = rows.iter().map(|row| row.dep.name.as_str()).collect();
        assert_eq!(names, vec!["express", "b-direct", "a-transitive"]);
        assert_eq!(rows[0].severity, Some(Severity::Critical));
        assert_eq!(rows[0].fix, Some("4.21.1"));
        assert_eq!(rows[0].advisories.len(), 2);

        // Les alertes d'un autre projet ne comptent pas.
        let other = rows_for_other(&deps, &alerts);
        assert!(other.iter().all(|row| !row.is_vulnerable()));
    }

    fn rows_for_other<'a>(deps: &'a [Dependency], alerts: &'a [Advisory]) -> Vec<Row<'a>> {
        rows(deps, alerts, &PathBuf::from("/tmp/autre"))
    }

    #[test]
    fn source_sentence_names_lockfiles_and_unresolved() {
        let mut manifest_only = dep("x", "1.0.0", true);
        manifest_only.resolved = false;
        manifest_only.source = "api/package.json".into();
        let sentence = source_sentence(&[dep("a", "1.0.0", true), manifest_only]);
        assert!(sentence.starts_with("Versions installées, lues dans package-lock.json."));
        assert!(sentence.contains("1 dépendance sans version exacte"));
    }
}

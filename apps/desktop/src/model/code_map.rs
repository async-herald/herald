//! Carte du code (onglet Général) : les fichiers analysés regroupés par
//! dossier, avec leurs problèmes, et leur disposition en « treemap » (aire
//! proportionnelle au nombre de fichiers).

use super::{AnalysisReport, Severity, rule_title};

/// Dossiers montrés au plus ; les suivants sont réunis dans « autres ».
pub const MAX_FOLDERS: usize = 12;
/// Nom des fichiers à la racine du projet.
pub const ROOT: &str = "racine";
pub const OTHERS: &str = "autres dossiers";

/// Un dossier de la carte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub name: String,
    pub files: usize,
    /// Problèmes ouverts (hors règles « à revoir » et corrections cochées).
    pub issues: usize,
    pub critical: usize,
    /// Titre de la règle la plus fréquente du dossier.
    pub top: Option<String>,
    /// Premier problème le plus grave : fichier et ligne (pour l'ouvrir).
    pub first: Option<(String, Option<u32>)>,
}

impl Folder {
    /// Problèmes par fichier.
    pub fn density(&self) -> f32 {
        self.issues as f32 / self.files.max(1) as f32
    }
}

/// Dossier d'un fichier : ses deux premiers niveaux (`src/game` pour
/// `src/game/Enemy.ts`), son premier pour `src/main.ts`, « racine » sinon.
pub fn folder_of(path: &str) -> String {
    let dirs: Vec<&str> = path.split('/').collect();
    match dirs.len() {
        0 | 1 => ROOT.to_owned(),
        2 => dirs[0].to_owned(),
        _ => format!("{}/{}", dirs[0], dirs[1]),
    }
}

/// Dossiers des fichiers analysés, les plus fournis d'abord (puis par nom),
/// avec les problèmes ouverts du rapport.
pub fn folders(paths: &[String], report: &AnalysisReport) -> Vec<Folder> {
    let mut out: Vec<Folder> = Vec::new();
    let index = |out: &mut Vec<Folder>, name: String| -> usize {
        match out.iter().position(|f| f.name == name) {
            Some(ix) => ix,
            None => {
                out.push(Folder {
                    name,
                    files: 0,
                    issues: 0,
                    critical: 0,
                    top: None,
                    first: None,
                });
                out.len() - 1
            }
        }
    };
    for path in paths {
        let ix = index(&mut out, folder_of(path));
        out[ix].files += 1;
    }

    // Problèmes, par dossier : compte, critiques, règle la plus fréquente,
    // premier problème le plus grave.
    let mut rules: Vec<Vec<(String, usize)>> = vec![Vec::new(); out.len()];
    let mut worst: Vec<Option<Severity>> = vec![None; out.len()];
    for (ix, issue) in report.issues.iter().enumerate() {
        if issue.review || report.is_resolved(ix) {
            continue;
        }
        // Relevés hors fichier (historique Git) : pas sur la carte.
        let Some(folder) = out.iter().position(|f| f.name == folder_of(&issue.file)) else {
            continue;
        };
        let f = &mut out[folder];
        f.issues += 1;
        if issue.severity == Severity::Critical {
            f.critical += 1;
        }
        if worst[folder].is_none_or(|w| issue.severity < w) {
            worst[folder] = Some(issue.severity);
            f.first = Some((issue.file.to_string(), issue.line));
        }
        match rules[folder]
            .iter_mut()
            .find(|(rule, _)| *rule == issue.rule.as_ref())
        {
            Some(entry) => entry.1 += 1,
            None => rules[folder].push((issue.rule.to_string(), 1)),
        }
    }
    for (folder, counts) in out.iter_mut().zip(rules) {
        folder.top = counts
            .into_iter()
            .max_by_key(|(_, n)| *n)
            .map(|(rule, _)| rule_title(&rule));
    }

    out.sort_by(|a, b| b.files.cmp(&a.files).then_with(|| a.name.cmp(&b.name)));
    if out.len() > MAX_FOLDERS {
        let rest = out.split_off(MAX_FOLDERS - 1);
        out.push(Folder {
            name: OTHERS.to_owned(),
            files: rest.iter().map(|f| f.files).sum(),
            issues: rest.iter().map(|f| f.issues).sum(),
            critical: rest.iter().map(|f| f.critical).sum(),
            top: None,
            first: rest.iter().find_map(|f| f.first.clone()),
        });
    }
    out
}

/// Rectangle en fractions de la carte (0 à 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Disposition « squarified » (Bruls, Huizing, van Wijk) : des rectangles
/// d'aire proportionnelle à `weights` (triés du plus grand au plus petit),
/// aussi carrés que possible, dans une carte de rapport `aspect` (largeur sur
/// hauteur). Les rectangles sont rendus en fractions de la carte.
pub fn squarify(weights: &[f32], aspect: f32) -> Vec<Rect> {
    let total: f32 = weights.iter().sum();
    if weights.is_empty() || total <= 0. {
        return Vec::new();
    }
    // Calcul dans un rectangle aspect × 1, ramené en fractions à la fin.
    let areas: Vec<f32> = weights.iter().map(|w| w / total * aspect).collect();
    let mut out: Vec<Rect> = Vec::with_capacity(areas.len());
    let (mut x, mut y, mut w, mut h) = (0., 0., aspect, 1.);

    // Pire rapport largeur/hauteur d'une rangée posée le long d'un côté `side`.
    let worst = |row: &[f32], side: f32| -> f32 {
        let sum: f32 = row.iter().sum();
        let max = row.iter().copied().fold(0., f32::max);
        let min = row.iter().copied().fold(f32::MAX, f32::min);
        let side2 = side * side;
        f32::max(side2 * max / (sum * sum), (sum * sum) / (side2 * min))
    };

    let mut row: Vec<f32> = Vec::new();
    let mut ix = 0;
    while ix < areas.len() {
        let side = w.min(h);
        let mut candidate = row.clone();
        candidate.push(areas[ix]);
        if row.is_empty() || worst(&candidate, side) <= worst(&row, side) {
            row = candidate;
            ix += 1;
            continue;
        }
        place(&row, &mut x, &mut y, &mut w, &mut h, &mut out);
        row.clear();
    }
    if !row.is_empty() {
        place(&row, &mut x, &mut y, &mut w, &mut h, &mut out);
    }
    out.iter()
        .map(|r| Rect {
            x: r.x / aspect,
            y: r.y,
            w: r.w / aspect,
            h: r.h,
        })
        .collect()
}

/// Pose une rangée le long du petit côté du rectangle restant, puis le réduit.
fn place(row: &[f32], x: &mut f32, y: &mut f32, w: &mut f32, h: &mut f32, out: &mut Vec<Rect>) {
    let sum: f32 = row.iter().sum();
    if *w >= *h {
        // Colonne à gauche.
        let width = sum / *h;
        let mut cy = *y;
        for area in row {
            let height = area / width;
            out.push(Rect {
                x: *x,
                y: cy,
                w: width,
                h: height,
            });
            cy += height;
        }
        *x += width;
        *w -= width;
    } else {
        // Ligne en haut.
        let height = sum / *w;
        let mut cx = *x;
        for area in row {
            let width = area / height;
            out.push(Rect {
                x: cx,
                y: *y,
                w: width,
                h: height,
            });
            cx += width;
        }
        *y += height;
        *h -= height;
    }
}

#[cfg(test)]
mod tests {
    use super::{Rect, folder_of, squarify};

    #[test]
    fn folders_keep_two_levels() {
        assert_eq!(folder_of("package.json"), "racine");
        assert_eq!(folder_of("src/main.ts"), "src");
        assert_eq!(folder_of("src/game/Enemy.ts"), "src/game");
        assert_eq!(folder_of("ios/App/App/AppDelegate.swift"), "ios/App");
    }

    #[test]
    fn treemap_areas_match_weights_and_fill_the_map() {
        let weights = [31., 19., 9., 5., 3., 3., 2., 1.];
        let rects = squarify(&weights, 1.7);
        assert_eq!(rects.len(), weights.len());
        let total: f32 = weights.iter().sum();
        let mut covered = 0.;
        for (rect, weight) in rects.iter().zip(weights) {
            let area = rect.w * rect.h;
            assert!((area - weight / total).abs() < 1e-4, "{rect:?}");
            assert!(rect.x >= -1e-4 && rect.y >= -1e-4);
            assert!(rect.x + rect.w <= 1. + 1e-4 && rect.y + rect.h <= 1. + 1e-4);
            covered += area;
        }
        assert!((covered - 1.).abs() < 1e-3);
    }

    #[test]
    fn a_single_folder_takes_the_whole_map() {
        assert_eq!(
            squarify(&[4.], 2.),
            vec![Rect {
                x: 0.,
                y: 0.,
                w: 1.,
                h: 1.
            }]
        );
        assert!(squarify(&[], 2.).is_empty());
    }
}

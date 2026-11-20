//! Règles maintenues par Herald Desktop (`rules/` à la racine du dépôt).
//!
//! Une règle de `rules/` remplace, à identifiant égal, celle du catalogue
//! (herald.codes ou moteur) : c'est ainsi qu'une règle corrigée après la mesure
//! de précision (`examples/precision.rs`) s'applique sans toucher au moteur.
//! `rules/precision.json` garde la précision mesurée de chaque règle de
//! sécurité ; sous [`MIN_PRECISION`], ses relevés sont « À revoir » et ne
//! comptent pas dans la note.

use std::collections::HashMap;
use std::sync::LazyLock;

use include_dir::{Dir, include_dir};
use serde::Deserialize;

static RULES_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/rules");

/// Précision en dessous de laquelle une règle passe « À revoir ».
pub const MIN_PRECISION: f32 = 0.9;

/// Sources `.herald` de l'app, par identifiant de règle.
pub fn sources() -> &'static HashMap<String, &'static str> {
    static SOURCES: LazyLock<HashMap<String, &'static str>> = LazyLock::new(|| {
        let mut out = HashMap::new();
        collect(&RULES_DIR, &mut out);
        out
    });
    &SOURCES
}

fn collect(dir: &'static Dir<'static>, out: &mut HashMap<String, &'static str>) {
    for file in dir.files() {
        if file.path().extension().and_then(|e| e.to_str()) != Some("herald") {
            continue;
        }
        let Some(source) = file.contents_utf8() else {
            continue;
        };
        match herald_engine::dsl::parse_meta(source) {
            Ok(meta) => {
                out.insert(meta.id, source);
            }
            Err(err) => eprintln!("[règles] {} : {err}", file.path().display()),
        }
    }
    for sub in dir.dirs() {
        collect(sub, out);
    }
}

/// Remplace dans `base` les règles que l'app maintient. Une source de `base`
/// illisible est gardée telle quelle (le moteur l'ignorera avec un log).
pub fn merge(base: Vec<String>) -> Vec<String> {
    let local = sources();
    base.into_iter()
        .map(|source| {
            herald_engine::dsl::parse_meta(&source)
                .ok()
                .and_then(|meta| local.get(&meta.id))
                .map_or(source, |local| (*local).to_owned())
        })
        .collect()
}

/// Catalogue intégré au moteur (hors ligne), avec les règles de l'app.
pub fn builtin_sources() -> Vec<String> {
    merge(
        herald_engine::dsl::builtin_rules()
            .into_iter()
            .map(|rule| rule.source.to_owned())
            .collect(),
    )
}

/// Mesure d'une règle sur le corpus (`rules/precision.json`).
#[derive(Clone, Debug, Deserialize)]
pub struct Measure {
    /// Relevés étiquetés « vrai » dans l'échantillon.
    pub true_positives: u32,
    /// Relevés étiquetés.
    pub labeled: u32,
    /// Relevés sur tout le corpus (échantillonnés pour l'étiquetage).
    #[serde(default)]
    pub total: u32,
}

impl Measure {
    pub fn precision(&self) -> Option<f32> {
        (self.labeled > 0).then(|| self.true_positives as f32 / self.labeled as f32)
    }
}

#[derive(Deserialize)]
struct PrecisionFile {
    rules: HashMap<String, Measure>,
}

pub fn measures() -> &'static HashMap<String, Measure> {
    static MEASURES: LazyLock<HashMap<String, Measure>> = LazyLock::new(|| {
        serde_json::from_str::<PrecisionFile>(include_str!("../../rules/precision.json"))
            .map(|file| file.rules)
            .unwrap_or_else(|err| {
                eprintln!("[règles] precision.json illisible : {err}");
                HashMap::new()
            })
    });
    &MEASURES
}

/// Règle trop imprécise sur le corpus : ses relevés sont à revoir, hors note.
pub fn needs_review(rule: &str) -> bool {
    measures()
        .get(rule)
        .and_then(Measure::precision)
        .is_some_and(|precision| precision < MIN_PRECISION)
}

/// Dossiers qui ne sont pas la surface d'attaque du projet : documentation,
/// exemples, démos, benchmarks, tests et fixtures.
const NON_PRODUCTION_DIRS: &[&str] = &[
    "doc",
    "docs",
    "docs_src",
    "documentation",
    "example",
    "examples",
    "sample",
    "samples",
    "demo",
    "demos",
    "bench",
    "benches",
    "benchmark",
    "benchmarks",
    "test",
    "tests",
    "__tests__",
    "spec",
    "testdata",
    "fixture",
    "fixtures",
    "_fixture",
    "__fixtures__",
];

/// Règles de sécurité gardées même hors production : un jeton au format
/// reconnu publié dans la documentation ou un test reste une fuite.
const EVERYWHERE: &[&str] = &["leaked-secret"];

/// Relevé de sécurité à écarter : fichier hors production (voir
/// [`NON_PRODUCTION_DIRS`]) ou code minifié / généré, que personne ne corrige
/// à la main. Mesuré sur le corpus : ces fichiers ne donnaient que des faux
/// positifs (`bench/labels/baseline.jsonl`).
pub fn outside_attack_surface(rule: &str, path: &str, code: &str) -> bool {
    if EVERYWHERE.contains(&rule) {
        return false;
    }
    let lower = path.to_ascii_lowercase();
    let mut segments = lower.split('/');
    let file = segments.next_back().unwrap_or_default();
    if segments.any(|dir| NON_PRODUCTION_DIRS.contains(&dir)) {
        return true;
    }
    // Fichier de test isolé (`api.test.ts`, `keypair_test.go`, `.env.test`) :
    // le moteur garde exprès les règles de secrets dans les tests, la mesure
    // n'y a trouvé que des clés et des jetons de test.
    herald_engine::prod_scope::is_non_prod_file(path)
        || file.starts_with(".env.test")
        || file.contains(".min.")
        || is_minified(code)
}

/// Relevé à écarter : hors surface d'attaque, ou relevé d'une règle écrite en
/// Rust dans le moteur (non remplaçable par `rules/`) qui ne porte sur aucun
/// secret, d'après les faux positifs mesurés :
/// - `committed-secret-file` sur un `.npmrc` sans jeton (`_authToken`, `_auth`) ;
/// - `committed-env-file` sur un fichier sans variable de secret renseignée.
pub fn dismissed(rule: &str, path: &str, code: &str) -> bool {
    if outside_attack_surface(rule, path, code) {
        return true;
    }
    let file = path.rsplit('/').next().unwrap_or(path);
    match rule {
        "committed-secret-file" if file == ".npmrc" => !code.contains("_auth"),
        "committed-env-file" => !code.lines().any(declares_secret),
        _ => false,
    }
}

/// `API_KEY=sk_live_…` : variable nommée comme un secret, valeur d'au moins
/// 8 caractères qui n'est pas un gabarit (`${…}`, `<…>`, `changeme`).
fn declares_secret(line: &str) -> bool {
    let line = line.trim();
    if line.starts_with('#') {
        return false;
    }
    let Some((key, value)) = line.split_once('=') else {
        return false;
    };
    let key = key
        .trim()
        .trim_start_matches("export ")
        .to_ascii_uppercase();
    let value = value.trim().trim_matches(['"', '\'']);
    let secret_name = [
        "SECRET",
        "TOKEN",
        "PASSWORD",
        "PASSWD",
        "API_KEY",
        "APIKEY",
        "PRIVATE_KEY",
    ]
    .iter()
    .any(|word| key.contains(word));
    let placeholder = value.starts_with("${")
        || value.starts_with('<')
        || ["changeme", "change_me", "xxx", "your_", "example"]
            .iter()
            .any(|word| value.to_ascii_lowercase().contains(word));
    secret_name && value.len() >= 8 && !placeholder
}

/// Code minifié : très longues lignes en moyenne (bundle, sortie de build).
fn is_minified(code: &str) -> bool {
    let lines = code.lines().count().max(1);
    let longest = code.lines().map(str::len).max().unwrap_or(0);
    longest > 1_000 && code.len() / lines > 250
}

/// Cas du bloc `tests { lang { "code" => fail|pass } }` d'une source :
/// `(langage, code, doit être relevé)`. Même échappement que le moteur
/// (`\n`, `\t`, `\x` → `x`).
#[cfg(test)]
pub(crate) fn examples(source: &str) -> Vec<(String, String, bool)> {
    let mut out = Vec::new();
    let Some(start) = source.find("tests {") else {
        return out;
    };
    let mut lang = None;
    for line in source[start..].lines().skip(1) {
        let line = line.trim();
        if let Some(name) = line.strip_suffix('{').map(str::trim) {
            lang = Some(name.to_owned());
            continue;
        }
        let Some(rest) = line.strip_prefix('"') else {
            continue;
        };
        let Some((code, verdict)) = rest.rsplit_once("=>") else {
            continue;
        };
        let code = code.trim_end().strip_suffix('"').unwrap_or(code);
        let mut text = String::new();
        let mut chars = code.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => text.push('\n'),
                    Some('t') => text.push('\t'),
                    Some(other) => text.push(other),
                    None => {}
                }
            } else {
                text.push(c);
            }
        }
        if let Some(lang) = &lang {
            out.push((lang.clone(), text, verdict.trim() == "fail"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use herald_engine::language::Language;

    /// Chaque règle de `rules/` relève ses cas `fail` et aucun de ses cas
    /// `pass` (dont les faux positifs trouvés sur le corpus).
    #[test]
    fn every_local_rule_passes_its_examples() {
        let mut failures = Vec::new();
        let mut count = 0;
        for (id, source) in sources() {
            for (lang, code, should_flag) in examples(source) {
                let language = Language::from_name(&lang).expect("langage connu");
                let flagged = herald_engine::dsl::run_source(source, &code, language)
                    .expect("règle valide")
                    .iter()
                    .any(|issue| &issue.rule == id);
                count += 1;
                if flagged != should_flag {
                    failures.push(format!(
                        "{id} [{lang}] attendu {} : {code}",
                        if should_flag { "fail" } else { "pass" }
                    ));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{count} cas, échecs :\n{}",
            failures.join("\n")
        );
    }

    /// Chaque fichier `.herald` de `rules/` se parse : une règle invalide serait
    /// ignorée sans bruit, ses cas de test avec.
    #[test]
    fn every_rule_file_parses() {
        fn count(dir: &Dir) -> usize {
            dir.files()
                .filter(|f| f.path().extension().and_then(|e| e.to_str()) == Some("herald"))
                .count()
                + dir.dirs().map(count).sum::<usize>()
        }
        assert_eq!(sources().len(), count(&RULES_DIR));
        for (id, source) in sources() {
            assert!(!examples(source).is_empty(), "{id} : aucun cas de test");
        }
    }

    /// Une règle de l'app remplace une règle existante : pas de faute de frappe
    /// dans l'identifiant, qui la ferait ignorer silencieusement.
    #[test]
    fn local_rules_replace_engine_rules() {
        let engine: Vec<String> = herald_engine::dsl::builtin_rules()
            .into_iter()
            .map(|rule| rule.id)
            .collect();
        for id in sources().keys() {
            assert!(engine.contains(id), "règle inconnue du moteur : {id}");
        }
    }

    #[test]
    fn examples_are_parsed_like_the_engine() {
        let source = "rule r {\n  tests {\n    js {\n      \"a(\\\"x\\\")\\nb()\" => fail\n      \"c()\" => pass\n    }\n  }\n}\n";
        assert_eq!(
            examples(source),
            vec![
                ("js".into(), "a(\"x\")\nb()".into(), true),
                ("js".into(), "c()".into(), false),
            ]
        );
    }

    #[test]
    fn attack_surface_excludes_docs_examples_and_bundles() {
        assert!(outside_attack_surface(
            "sql_injection",
            "docs_src/security/app.py",
            ""
        ));
        assert!(!outside_attack_surface(
            "sql_injection",
            "src/db/query.ts",
            ""
        ));
        assert!(outside_attack_surface(
            "private-key-block",
            "gin/testdata/key.pem",
            ""
        ));
        assert!(outside_attack_surface("no_eval", "static/app.min.js", ""));
        let bundle = format!("{}\n", "a".repeat(5_000));
        assert!(outside_attack_surface(
            "no_eval",
            "web/static/index-DA3b.js",
            &bundle
        ));
        assert!(!outside_attack_surface(
            "no_eval",
            "src/server/app.js",
            "eval(x)\n"
        ));
        assert!(!outside_attack_surface(
            "leaked-secret",
            "docs/setup.md",
            ""
        ));
        // « test » doit être un dossier entier, pas un morceau de nom.
        assert!(!outside_attack_surface(
            "no_eval",
            "src/testable/run.js",
            ""
        ));
    }

    #[test]
    fn engine_rules_are_dismissed_without_a_secret() {
        assert!(outside_attack_surface(
            "private-key-block",
            "server/S3Storage.cloudfront.test.ts",
            ""
        ));
        assert!(outside_attack_surface(
            "private-key-block",
            "modules/util/keypair_test.go",
            ""
        ));
        assert!(dismissed(
            "committed-secret-file",
            ".npmrc",
            "save-exact=true\n"
        ));
        assert!(!dismissed(
            "committed-secret-file",
            ".npmrc",
            "//registry.npmjs.org/:_authToken=npm_abc123\n"
        ));
        assert!(dismissed(
            "committed-env-file",
            ".env.production",
            "MODE=\"production\"\n"
        ));
        assert!(dismissed("committed-env-file", ".envrc", "use flake\n"));
        assert!(dismissed(
            "committed-env-file",
            ".env",
            "API_KEY=${API_KEY}\n"
        ));
        assert!(!dismissed(
            "committed-env-file",
            ".env",
            "STRIPE_SECRET_KEY=sk_live_51Hx9a\n"
        ));
        assert!(dismissed(
            "committed-env-file",
            ".env.test",
            "SECRET_KEY=F0E5AD933D7F6FD8\n"
        ));
    }

    /// `rules/precision.json` se lit, ne cite que des règles connues, et porte
    /// les décisions de la dernière mesure.
    #[test]
    fn precision_file_drives_review() {
        let known: Vec<String> = herald_engine::rules::list()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert!(!measures().is_empty());
        for id in measures().keys() {
            assert!(
                known.contains(id),
                "règle inconnue dans precision.json : {id}"
            );
        }
        assert!(!needs_review("tls_verification_disabled"));
        assert!(needs_review("weak_crypto"));
        assert!(needs_review("taint-command-injection"));
        // Pas de relevé sur le corpus : non mesurée, reste active.
        assert!(!needs_review("sql_injection"));
    }

    #[test]
    fn measures_decide_review() {
        let measure = Measure {
            true_positives: 8,
            labeled: 10,
            total: 40,
        };
        assert_eq!(measure.precision(), Some(0.8));
    }
}

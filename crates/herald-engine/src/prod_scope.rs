//! Portée « production » : certaines règles (fiabilité — panic/unwrap, erreurs
//! avalées…) n'ont de sens que sur le code qui tourne en prod. Les faire crier
//! sur du code de test est un faux positif pur : `.unwrap()` dans un test ou un
//! benchmark est parfaitement idiomatique (on VEUT qu'il panique si l'invariant
//! casse). Ce module identifie le code non-prod pour en retirer ces relevés.
//!
//! Deux niveaux :
//!   * le fichier entier (`tests/`, `benches/`, `examples/`, `*_test.rs`, …) ;
//!   * en Rust, les modules `#[cfg(test)]` et fonctions `#[test]`/`#[tokio::test]`
//!     *inline* dans un fichier de prod (le cas de très loin le plus fréquent).

use std::sync::LazyLock;

use regex::Regex;
use tree_sitter::{Node, Tree};

use crate::category::Category;
use crate::language::Language;
use crate::types::Issue;

/// Détection de secret au format reconnu (bloc de clé privée, `.env`
/// committé, fichier de secret connu) — active sur TOUS les fichiers, y
/// compris les tests : un vrai secret oublié dans un fixture reste un vrai
/// secret, peu importe le dossier. Différent des heuristiques (ex.
/// `hardcoded-password`, qui devine à partir d'un nom de variable et
/// produit beaucoup de faux positifs sur des mots de passe de test bidons) —
/// celles-là restent soumises à la même règle que le reste sur un fichier de
/// test entier : seule `Category::Testing` en sort.
const HARD_SECRET_RULES: &[&str] = &["committed-env-file", "committed-secret-file", "private-key-block"];

/// Règles dont les relevés ne comptent QUE sur du code de production. À
/// l'origine limité à l'ex-catégorie « fiabilité » (retirée, ces règles sont
/// maintenant rangées en `Category::Quality`), étendu à Pattern/Performance :
/// un fichier de test n'a pas à respecter les mêmes principes d'architecture
/// qu'un fichier de prod (SOLID, couplage), et du code de setup dans un test
/// n'est jamais un chemin critique en prod. Sécurité, dépendances et le reste
/// de la qualité (code mort, secrets, nommage) restent actifs partout, y
/// compris dans les tests — ce filtre doit rester ciblé sur ces règles
/// précisément, pas sur tout ce qui n'est pas « sécurité ».
const PROD_ONLY_RULES: &[&str] = &[
    "img-missing-alt",
    "compare-nan",
    "http-no-timeout",
    "assignment-in-condition",
    "loose-equality",
    "return-in-finally",
    "no-unwrap-in-prod",
    "swallowed-error",
    "unhandled-promise",
    "unsafe-negation",
    "php-error-suppression",
    // Pattern (architecture) — pas de sens à juger un fichier de test sur les
    // mêmes critères qu'un module de prod.
    "import-cycle",
    "high-coupling",
    "deep-relative-import",
    // Performance — du code de setup/fixture n'est pas un chemin critique.
    "io-in-loop",
    "quadratic-loop",
];

fn is_prod_only(rule: &str) -> bool {
    PROD_ONLY_RULES.contains(&rule)
}

/// Un segment de chemin qui désigne, à lui seul, un fichier non-prod.
static NON_PROD_SEGMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(tests?|benches|bench|examples?|__tests__|__mocks__|spec|specs|e2e|fixtures?|testdata)$")
        .unwrap()
});

/// Un nom de fichier qui désigne, à lui seul, un fichier de test.
static NON_PROD_FILE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(^test_.*|.*_tests?$|.*\.(test|spec)$|.*_test$)").unwrap()
});

/// Vrai si le fichier entier est du code non-prod (test, bench, exemple, fixture),
/// d'après son chemin — indépendamment du langage.
pub fn is_non_prod_file(path: &str) -> bool {
    let norm = path.replace('\\', "/");
    // Un segment de dossier suffit (`crate/tests/foo.rs`, `benches/db.rs`).
    if norm
        .split('/')
        .any(|seg| NON_PROD_SEGMENT.is_match(seg))
    {
        return true;
    }
    // Sinon, le nom de fichier sans extension (`db_test.rs`, `foo.spec.ts`).
    norm.rsplit('/')
        .next()
        .map(strip_known_ext)
        .is_some_and(|stem| NON_PROD_FILE.is_match(stem))
}

/// Retire l'extension de langage connue pour ne raisonner que sur le nom logique
/// (`foo.spec.ts` → `foo.spec`, `db_test.rs` → `db_test`).
fn strip_known_ext(name: &str) -> &str {
    name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name)
}

/// Retire, en place, les relevés hors-propos sur du code non-prod :
///   * fichier de test entier (`*.test.ts`, `tests/`, …) — ne garde QUE
///     `Category::Testing` et la détection de secret au format reconnu ;
///     tout le reste (sécurité heuristique, qualité, pattern, perf...) ne
///     compte que dans la catégorie Tests, pas ailleurs ;
///   * bloc `#[cfg(test)]`/`#[test]` inline dans un fichier de prod — ne
///     retire que les règles « prod-only » ciblées (portée plus étroite,
///     le reste du fichier étant réellement de la prod).
pub fn retain_prod(
    issues: &mut Vec<Issue>,
    path: &str,
    tree: Option<&Tree>,
    code: &str,
    lang: Option<Language>,
) {
    if is_non_prod_file(path) {
        issues.retain(|i| i.category == Category::Testing || HARD_SECRET_RULES.contains(&i.rule.as_str()));
        return;
    }

    // Rien de prod-only à filtrer ? on ne touche à rien (cas courant).
    if !issues.iter().any(|i| is_prod_only(&i.rule)) {
        return;
    }

    // Fichier de prod : ne retirer que les relevés tombant dans une portée de
    // test inline. Aujourd'hui détecté pour Rust (`#[cfg(test)]` / `#[test]`).
    let ranges = match (tree, lang) {
        (Some(t), Some(Language::Rust)) => test_line_ranges_rust(t, code),
        _ => Vec::new(),
    };
    if ranges.is_empty() {
        return;
    }
    issues.retain(|i| !(is_prod_only(&i.rule) && in_any_range(i.line, &ranges)));
}

fn in_any_range(line: usize, ranges: &[(usize, usize)]) -> bool {
    ranges.iter().any(|&(a, b)| line >= a && line <= b)
}

/// Plage `#[cfg(test)]` dans un attribut : `cfg(...)` contenant le mot `test`
/// (couvre `cfg(test)` et `cfg(all(test, …))`).
static CFG_TEST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"cfg\s*\(.*\btest\b").unwrap());
/// Attribut de test sur une fonction : `#[test]`, `#[tokio::test]`, `#[…::test]`.
static TEST_ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#!?\[\s*(?:\w+\s*::\s*)*test\s*\]$").unwrap());

fn is_test_attribute(text: &str) -> bool {
    let t = text.trim();
    CFG_TEST.is_match(t) || TEST_ATTR.is_match(t)
}

/// Lignes (1-based, inclusives) couvertes par un module `#[cfg(test)]` ou une
/// fonction `#[test]` en Rust. Un relevé de fiabilité dans une de ces plages est
/// du code de test et ne compte pas.
fn test_line_ranges_rust(tree: &Tree, code: &str) -> Vec<(usize, usize)> {
    let src = code.as_bytes();
    let mut ranges = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == "attribute_item"
            && node.utf8_text(src).is_ok_and(is_test_attribute)
            && let Some(item) = annotated_item(node)
        {
            ranges.push((item.start_position().row + 1, item.end_position().row + 1));
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
    ranges
}

/// L'item (module / fonction) annoté par cet `attribute_item`. Selon la grammaire,
/// l'attribut est soit un enfant en tête de l'item, soit un frère qui le précède —
/// on gère les deux.
fn annotated_item<'a>(attr: Node<'a>) -> Option<Node<'a>> {
    let is_item = |n: &Node| matches!(n.kind(), "mod_item" | "function_item");
    if let Some(parent) = attr.parent()
        && is_item(&parent)
    {
        return Some(parent);
    }
    let mut sib = attr.next_named_sibling();
    while let Some(n) = sib {
        if n.kind() == "attribute_item" {
            sib = n.next_named_sibling();
            continue;
        }
        return is_item(&n).then_some(n);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;
    use crate::types::Severity;

    fn issue(line: usize, rule: &str) -> Issue {
        issue_in(line, rule, Category::Quality)
    }

    fn issue_in(line: usize, rule: &str, category: Category) -> Issue {
        Issue {
            rule: rule.into(),
            category,
            severity: Severity::Error,
            message: String::new(),
            line,
            column: 1,
        }
    }

    #[test]
    fn detects_non_prod_files() {
        assert!(is_non_prod_file("tests/streaming_test.rs"));
        assert!(is_non_prod_file("benches/database.rs"));
        assert!(is_non_prod_file("examples/demo.rs"));
        assert!(is_non_prod_file("src/foo/__tests__/bar.js"));
        assert!(is_non_prod_file("db_test.rs"));
        assert!(is_non_prod_file("src/api.test.ts"));
        assert!(is_non_prod_file("src/api.spec.ts"));
        // Prod bien réel — ne doit PAS matcher.
        assert!(!is_non_prod_file("src/main.rs"));
        assert!(!is_non_prod_file("src/database.rs"));
        assert!(!is_non_prod_file("crates/herald-engine/src/lib.rs"));
        assert!(!is_non_prod_file("src/testable.rs")); // "test" n'est pas un segment entier
    }

    fn ranges(code: &str) -> Vec<(usize, usize)> {
        let tree = parser::parse(code, Language::Rust).unwrap();
        test_line_ranges_rust(&tree, code)
    }

    #[test]
    fn finds_cfg_test_module_range() {
        let code = "\
fn prod() { x.unwrap(); }

#[cfg(test)]
mod tests {
    #[test]
    fn t() { y.unwrap(); }
}
";
        let r = ranges(code);
        // Une plage couvrant le module de test (lignes 3..7 environ).
        assert!(r.iter().any(|&(a, b)| a <= 4 && b >= 6), "ranges={r:?}");
        // La ligne 1 (prod) n'est dans aucune plage.
        assert!(!in_any_range(1, &r));
        // La ligne 6 (unwrap de test) est couverte.
        assert!(in_any_range(6, &r));
    }

    #[test]
    fn retain_drops_test_unwrap_keeps_prod() {
        let code = "\
fn prod() { x.unwrap(); }

#[cfg(test)]
mod tests {
    fn t() { y.unwrap(); }
}
";
        let tree = parser::parse(code, Language::Rust);
        let mut issues = vec![
            issue(1, "no-unwrap-in-prod"), // prod → gardé
            issue(5, "no-unwrap-in-prod"), // dans cfg(test) → retiré
        ];
        retain_prod(&mut issues, "src/lib.rs", tree.as_ref(), code, Some(Language::Rust));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].line, 1);
    }

    #[test]
    fn retain_leaves_non_prod_only_untouched_in_prod_file() {
        // Fichier de PROD (pas de segment/nom de test) : sans portée de test
        // inline détectée, les règles qui ne sont pas prod-only ne bougent pas.
        let mut issues = vec![issue(10, "sql-injection"), issue(20, "long-line")];
        let before = issues.len();
        retain_prod(&mut issues, "src/database.rs", None, "", Some(Language::Rust));
        assert_eq!(issues.len(), before);
    }

    #[test]
    fn retain_keeps_only_testing_category_and_hard_secrets_in_test_file() {
        let mut issues = vec![
            issue_in(1, "import-cycle", Category::Pattern),
            issue_in(2, "hardcoded-password", Category::Security), // heuristique → retiré
            issue_in(3, "sql-injection", Category::Security),      // heuristique → retiré
            issue_in(4, "long-line", Category::Quality),
            issue_in(5, "committed-secret-file", Category::Security), // format reconnu → gardé
            issue_in(6, "private-key-block", Category::Security),     // format reconnu → gardé
            issue_in(7, "flaky-test", Category::Testing),              // catégorie Tests → gardé
        ];
        retain_prod(&mut issues, "src/api.test.ts", None, "", None);
        let kept: Vec<&str> = issues.iter().map(|i| i.rule.as_str()).collect();
        assert_eq!(kept, vec!["committed-secret-file", "private-key-block", "flaky-test"]);
    }

    #[test]
    fn retain_treats_whole_non_prod_dirs_the_same_way() {
        let mut issues = vec![issue(10, "no-unwrap-in-prod"), issue(20, "swallowed-error")];
        retain_prod(&mut issues, "benches/database.rs", None, "", Some(Language::Rust));
        assert!(issues.is_empty());
    }
}

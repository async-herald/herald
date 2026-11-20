//! Catégorie Tests : les 4 règles (`no-focused-tests`, `no-skipped-tests`,
//! `no-empty-test`, `placeholder-assertion`) sont toutes en DSL pure — pas de
//! logique Rust ici pour l'analyse elle-même. Ce module n'existe que pour
//! `is_test_file`, utilisé pour savoir si la catégorie a quelque chose à
//! examiner : ces règles matchent des appels type `it(...)`/`describe(...)`
//! n'importe où, donc "zéro relevé" est indiscernable de "zéro fichier de
//! test" sans un signal indépendant — d'où ce détecteur par nom de fichier.

fn filename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Conventions de nommage des fichiers de test les plus répandues, tous
/// écosystèmes confondus (JS/TS, Python, Go, Java/Kotlin, C#, Ruby).
pub fn is_test_file(path: &str) -> bool {
    let name = filename(path).to_ascii_lowercase();
    let lower = path.to_ascii_lowercase();

    name.contains(".test.")
        || name.contains(".spec.")
        || name.ends_with("_test.go")
        || name.ends_with("_test.py")
        || name.starts_with("test_")
        || name.ends_with("test.java")
        || name.ends_with("tests.java")
        || name.ends_with("test.kt")
        || name.ends_with("tests.kt")
        || name.ends_with("tests.cs")
        || name.ends_with("_spec.rb")
        || name.ends_with("_test.rb")
        || lower.contains("/__tests__/")
        || lower.starts_with("__tests__/")
        || lower.contains("/tests/")
        || lower.starts_with("tests/")
        || lower.contains("/test/")
        || lower.starts_with("test/")
        || lower.contains("/spec/")
        || lower.starts_with("spec/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_test_files() {
        assert!(is_test_file("src/utils.test.ts"));
        assert!(is_test_file("src/utils.spec.js"));
        assert!(is_test_file("pkg/handler_test.go"));
        assert!(is_test_file("tests/test_auth.py"));
        assert!(is_test_file("src/main/java/UserServiceTest.java"));
        assert!(is_test_file("__tests__/App.js"));
        assert!(is_test_file("spec/models/user_spec.rb"));
    }

    #[test]
    fn rejects_regular_files() {
        assert!(!is_test_file("src/utils.ts"));
        assert!(!is_test_file("pkg/handler.go"));
        assert!(!is_test_file("src/contest.py"));
    }
}

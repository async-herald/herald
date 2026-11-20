//! Catégorie DevOps : hygiène des `Dockerfile` et des pipelines CI (GitHub
//! Actions / GitLab CI). Ni Dockerfile ni YAML n'ont de grammaire tree-sitter
//! enregistrée dans `language.rs` — pas de règle DSL `.herald` possible ici,
//! donc routage direct par nom de fichier plutôt que par `Language`, sur le
//! même principe que `semantic::detect_repo_hygiene` pour les règles `git`
//! non-DSL (`multiple-lockfiles`, `build-output-committed`).

mod dockerfile;
mod workflow;

use crate::config::Config;
use crate::types::Issue;

fn filename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `pub` : réutilisé par `herald-app` (`source/filter.rs`) pour décider quels
/// fichiers télécharger — une seule définition de "c'est un fichier DevOps",
/// partagée entre le fetch et l'analyse, pour ne pas les laisser diverger.
pub fn is_dockerfile(path: &str) -> bool {
    let name = filename(path);
    name == "Dockerfile" || name.starts_with("Dockerfile.") || name.ends_with(".dockerfile")
}

pub fn is_ci_workflow(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    (lower.contains("/.github/workflows/") || lower.starts_with(".github/workflows/"))
        && (lower.ends_with(".yml") || lower.ends_with(".yaml"))
        || matches!(filename(path), ".gitlab-ci.yml" | ".gitlab-ci.yaml")
}

pub fn check(path: &str, code: &str, config: &Config) -> Vec<Issue> {
    if is_dockerfile(path) {
        dockerfile::check(code, config)
    } else if is_ci_workflow(path) {
        workflow::check(code, config)
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_dockerfile_variants() {
        assert!(is_dockerfile("Dockerfile"));
        assert!(is_dockerfile("docker/Dockerfile"));
        assert!(is_dockerfile("Dockerfile.prod"));
        assert!(is_dockerfile("api.dockerfile"));
        assert!(!is_dockerfile("src/dockerfile.rs"));
    }

    #[test]
    fn recognizes_ci_workflows() {
        assert!(is_ci_workflow(".github/workflows/ci.yml"));
        assert!(is_ci_workflow("repo/.github/workflows/deploy.yaml"));
        assert!(is_ci_workflow(".gitlab-ci.yml"));
        assert!(!is_ci_workflow(".github/ISSUE_TEMPLATE/bug.yml"));
        assert!(!is_ci_workflow("config/values.yml"));
    }
}

//! Vérifications des pipelines CI (GitHub Actions / GitLab CI) — scan ligne à
//! ligne comme `dockerfile.rs` : pas de grammaire YAML enregistrée, donc pas
//! de règle DSL possible, juste des heuristiques regex.

use crate::category::Category;
use crate::config::Config;
use crate::types::{Issue, Severity};

fn issue(rule: &str, severity: Severity, message: String, line: usize) -> Issue {
    Issue { rule: rule.into(), category: Category::Devops, severity, message, line, column: 1 }
}

/// `uses: owner/repo@ref` où `ref` n'est pas un SHA de commit complet — une
/// action épinglée sur une branche/tag mouvant (`@main`, `@v4`) peut changer
/// de contenu sans prévenir : c'est le vecteur de l'attaque tj-actions 2025.
fn unpinned_action(code: &str, config: &Config, out: &mut Vec<Issue>) {
    if !config.is_rule_enabled("workflow-unpinned-action") {
        return;
    }
    for (i, line) in code.lines().enumerate() {
        // `- uses: owner/repo@ref` : le tiret de liste YAML précède parfois la clé.
        let trimmed = line.trim_start().trim_start_matches('-').trim_start();
        let Some(rest) = trimmed.strip_prefix("uses:") else { continue };
        let action = rest.trim().trim_matches('"').trim_matches('\'');
        let Some((_, ref_part)) = action.rsplit_once('@') else { continue };
        let is_full_sha = ref_part.len() == 40 && ref_part.chars().all(|c| c.is_ascii_hexdigit());
        if !is_full_sha {
            out.push(issue(
                "workflow-unpinned-action",
                Severity::Warning,
                format!("Action non épinglée sur un commit ({action}) — utilise le SHA complet, pas une branche/tag qui peut être réécrit"),
                i + 1,
            ));
        }
    }
}

/// `permissions: write-all` — le token du job peut tout modifier sur le repo
/// (contenu, PR, releases…) plutôt que le strict nécessaire.
fn write_all_permissions(code: &str, config: &Config, out: &mut Vec<Issue>) {
    if !config.is_rule_enabled("workflow-write-all-permissions") {
        return;
    }
    for (i, line) in code.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed == "permissions: write-all" || trimmed.ends_with(": write-all") {
            out.push(issue(
                "workflow-write-all-permissions",
                Severity::Error,
                "Permissions \"write-all\" — donne un accès en écriture bien plus large que nécessaire au token du job".to_string(),
                i + 1,
            ));
        }
    }
}

/// Un secret imprimé dans un step `run:` (`echo ${{ secrets.X }}`) : il finit
/// en clair dans les logs du job, visibles par quiconque a accès au repo.
fn secret_in_run(code: &str, config: &Config, out: &mut Vec<Issue>) {
    if !config.is_rule_enabled("workflow-secret-in-run") {
        return;
    }
    for (i, line) in code.lines().enumerate() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("echo") && lower.contains("secrets.") {
            out.push(issue(
                "workflow-secret-in-run",
                Severity::Error,
                "Secret potentiellement imprimé dans les logs du job (echo + secrets.*) — les logs restent lisibles par quiconque a accès au repo".to_string(),
                i + 1,
            ));
        }
    }
}

/// `pull_request_target` déclenche le workflow avec les secrets du repo
/// cible même sur une PR d'un fork — dangereux si le job checkout le code de
/// la PR (le contributeur peut alors exécuter du code arbitraire avec les
/// secrets). Simple signal "à vérifier", pas une détection précise du
/// checkout — trop de variantes YAML pour un regex fiable.
fn pull_request_target(code: &str, config: &Config, out: &mut Vec<Issue>) {
    if !config.is_rule_enabled("workflow-pull-request-target") {
        return;
    }
    for (i, line) in code.lines().enumerate() {
        if line.trim_start().starts_with("pull_request_target") {
            out.push(issue(
                "workflow-pull-request-target",
                Severity::Info,
                "Déclencheur pull_request_target — a accès aux secrets même depuis une PR d'un fork ; vérifie que le job ne checkout/exécute jamais le code de la PR sans restriction".to_string(),
                i + 1,
            ));
        }
    }
}

pub fn check(code: &str, config: &Config) -> Vec<Issue> {
    let mut out = Vec::new();
    unpinned_action(code, config, &mut out);
    write_all_permissions(code, config, &mut out);
    secret_in_run(code, config, &mut out);
    pull_request_target(code, config, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_unpinned_action() {
        let out = check("steps:\n  - uses: actions/checkout@main", &Config::default());
        assert!(out.iter().any(|i| i.rule == "workflow-unpinned-action"));
    }

    #[test]
    fn accepts_sha_pinned_action() {
        let sha = "a".repeat(40);
        let code = format!("steps:\n  - uses: actions/checkout@{sha}");
        let out = check(&code, &Config::default());
        assert!(!out.iter().any(|i| i.rule == "workflow-unpinned-action"));
    }

    #[test]
    fn flags_write_all_permissions() {
        let out = check("permissions: write-all\njobs:\n  build:\n    runs-on: ubuntu-latest", &Config::default());
        assert!(out.iter().any(|i| i.rule == "workflow-write-all-permissions"));
    }

    #[test]
    fn flags_secret_echoed() {
        let out = check("run: echo ${{ secrets.API_KEY }}", &Config::default());
        assert!(out.iter().any(|i| i.rule == "workflow-secret-in-run"));
    }

    #[test]
    fn flags_pull_request_target() {
        let out = check("on:\n  pull_request_target:", &Config::default());
        assert!(out.iter().any(|i| i.rule == "workflow-pull-request-target"));
    }

    #[test]
    fn clean_workflow_is_silent() {
        let sha = "b".repeat(40);
        let code = format!(
            "on:\n  push:\npermissions:\n  contents: read\njobs:\n  build:\n    steps:\n      - uses: actions/checkout@{sha}"
        );
        assert!(check(&code, &Config::default()).is_empty());
    }
}

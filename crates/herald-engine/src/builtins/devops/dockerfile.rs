//! Vérifications `Dockerfile` — scan ligne à ligne, comme les règles `git`
//! (`branch:`/`commit:`) : pas de grammaire tree-sitter pour Dockerfile, donc
//! pas de règle DSL `.herald` possible ici, juste des heuristiques regex.

use crate::category::Category;
use crate::config::Config;
use crate::types::{Issue, Severity};

fn issue(rule: &str, severity: Severity, message: String, line: usize) -> Issue {
    Issue { rule: rule.into(), category: Category::Devops, severity, message, line, column: 1 }
}

/// `FROM <image>:latest` — tag flottant, la build n'est plus reproductible et
/// peut changer de contenu sans prévenir entre deux runs.
fn latest_tag(code: &str, config: &Config, out: &mut Vec<Issue>) {
    if !config.is_rule_enabled("dockerfile-latest-tag") {
        return;
    }
    for (i, line) in code.lines().enumerate() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("FROM ") else { continue };
        let image = rest.split_whitespace().next().unwrap_or("");
        if image.ends_with(":latest") {
            out.push(issue(
                "dockerfile-latest-tag",
                Severity::Warning,
                format!("Image de base non figée ({image}) — épingle une version précise pour une build reproductible"),
                i + 1,
            ));
        }
    }
}

/// Aucune instruction `USER` : l'image tourne root par défaut.
fn root_user(code: &str, config: &Config, out: &mut Vec<Issue>) {
    if !config.is_rule_enabled("dockerfile-root-user") {
        return;
    }
    let has_user = code.lines().any(|l| l.trim_start().starts_with("USER "));
    if !has_user {
        out.push(issue(
            "dockerfile-root-user",
            Severity::Info,
            "Aucune instruction USER — le conteneur tourne root par défaut, ajoute un utilisateur dédié".to_string(),
            1,
        ));
    }
}

/// `ENV`/`ARG` avec un nom qui sent le secret et une valeur en clair (pas une
/// interpolation `${...}` ni vide) — un secret dans l'image est lisible par
/// quiconque a accès aux layers, même après suppression du fichier.
fn hardcoded_secret(code: &str, config: &Config, out: &mut Vec<Issue>) {
    if !config.is_rule_enabled("dockerfile-hardcoded-secret") {
        return;
    }
    const NEEDLES: &[&str] = &["PASSWORD", "SECRET", "TOKEN", "API_KEY", "PRIVATE_KEY", "ACCESS_KEY"];
    for (i, line) in code.lines().enumerate() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("ENV ").or_else(|| trimmed.strip_prefix("ARG ")) else { continue };
        let Some((name, value)) = rest.split_once('=').or_else(|| rest.split_once(' ')) else { continue };
        let name = name.trim();
        let value = value.trim().trim_matches('"').trim_matches('\'');
        let looks_secret = NEEDLES.iter().any(|n| name.to_ascii_uppercase().contains(n));
        let is_literal = !value.is_empty() && !value.starts_with("${") && !value.starts_with('$');
        if looks_secret && is_literal {
            out.push(issue(
                "dockerfile-hardcoded-secret",
                Severity::Error,
                format!("Secret en clair dans l'image ({name}) — passe-le en secret de build (`--secret`) ou en variable d'environnement au run, jamais figé dans un layer"),
                i + 1,
            ));
        }
    }
}

/// Aucune instruction `HEALTHCHECK` — l'orchestrateur (k8s, swarm…) ne peut
/// pas détecter un conteneur bloqué mais toujours "up".
fn missing_healthcheck(code: &str, config: &Config, out: &mut Vec<Issue>) {
    if !config.is_rule_enabled("dockerfile-missing-healthcheck") {
        return;
    }
    let has_healthcheck = code.lines().any(|l| l.trim_start().starts_with("HEALTHCHECK"));
    if !has_healthcheck {
        out.push(issue(
            "dockerfile-missing-healthcheck",
            Severity::Info,
            "Aucune instruction HEALTHCHECK — l'orchestrateur ne peut pas détecter un conteneur bloqué".to_string(),
            1,
        ));
    }
}

pub fn check(code: &str, config: &Config) -> Vec<Issue> {
    let mut out = Vec::new();
    latest_tag(code, config, &mut out);
    root_user(code, config, &mut out);
    hardcoded_secret(code, config, &mut out);
    missing_healthcheck(code, config, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_latest_tag() {
        let out = check("FROM node:latest\nUSER app\nHEALTHCHECK CMD true", &Config::default());
        assert!(out.iter().any(|i| i.rule == "dockerfile-latest-tag"));
    }

    #[test]
    fn accepts_pinned_tag() {
        let out = check("FROM node:20.11.0-slim\nUSER app\nHEALTHCHECK CMD true", &Config::default());
        assert!(!out.iter().any(|i| i.rule == "dockerfile-latest-tag"));
    }

    #[test]
    fn flags_missing_user() {
        let out = check("FROM node:20\nHEALTHCHECK CMD true", &Config::default());
        assert!(out.iter().any(|i| i.rule == "dockerfile-root-user"));
    }

    #[test]
    fn accepts_explicit_user() {
        let out = check("FROM node:20\nUSER app\nHEALTHCHECK CMD true", &Config::default());
        assert!(!out.iter().any(|i| i.rule == "dockerfile-root-user"));
    }

    #[test]
    fn flags_hardcoded_secret() {
        let out = check("FROM node:20\nENV API_KEY=sk_live_abc123\nUSER app", &Config::default());
        assert!(out.iter().any(|i| i.rule == "dockerfile-hardcoded-secret"));
    }

    #[test]
    fn accepts_interpolated_secret() {
        let out = check("FROM node:20\nENV API_KEY=${API_KEY}\nUSER app", &Config::default());
        assert!(!out.iter().any(|i| i.rule == "dockerfile-hardcoded-secret"));
    }

    #[test]
    fn flags_missing_healthcheck() {
        let out = check("FROM node:20\nUSER app", &Config::default());
        assert!(out.iter().any(|i| i.rule == "dockerfile-missing-healthcheck"));
    }

    #[test]
    fn disabled_rule_is_silent() {
        let mut config = Config::default();
        config.rules.disable.push("dockerfile-latest-tag".to_string());
        let out = check("FROM node:latest\nUSER app\nHEALTHCHECK CMD true", &config);
        assert!(!out.iter().any(|i| i.rule == "dockerfile-latest-tag"));
    }
}

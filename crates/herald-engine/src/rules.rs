use tree_sitter::Tree;

use crate::{
    builtins::devops, category::Category, config::Config, dsl, dsl::BuiltinRule, language::Language, types::Issue,
};

pub fn run_all(
    code: &str,
    tree: Option<&Tree>,
    language: Option<Language>,
    config: &Config,
) -> Vec<Issue> {
    dsl::run_all(code, tree, language, |id| config.is_rule_enabled(id))
}

/// Règles DevOps (Dockerfile / pipelines CI) : ni l'un ni l'autre n'ont de
/// grammaire tree-sitter enregistrée, donc routées par nom de fichier plutôt
/// que par la DSL — voir `builtins::devops`.
pub fn run_devops(path: &str, code: &str, config: &Config) -> Vec<Issue> {
    devops::check(path, code, config)
}

pub fn run_context(
    path: Option<&str>,
    branch: Option<&str>,
    commit: Option<&str>,
    config: &Config,
) -> Vec<Issue> {
    dsl::run_context(path, branch, commit, |id| config.is_rule_enabled(id))
}

/// Règles Rust "en dur" (pas de fichier `.herald`) : analyse cross-fichiers
/// (`semantic::mod`) ou routées par nom de fichier plutôt que par langage
/// (`builtins::devops` — ni Dockerfile ni YAML n'ont de grammaire tree-sitter).
/// Même forme que [`dsl::BuiltinRule`] pour que l'appelant (le seed DB dans
/// `herald-app`) puisse les traiter avec les règles DSL sans cas particulier —
/// sans ça elles restent invisibles du catalogue de règles du service, comme
/// `multiple-lockfiles`/`build-output-committed` l'étaient avant ce correctif.
pub fn extra_rules() -> Vec<BuiltinRule> {
    const RAW: &[(&str, Category, &str, &str)] = &[
        ("import-cycle", Category::Pattern, "warning", "Cycle d'imports entre plusieurs fichiers"),
        ("orphan-module", Category::Quality, "info", "Module jamais importé (code potentiellement mort)"),
        ("unused-export", Category::Quality, "info", "Export jamais importé ailleurs"),
        ("committed-env-file", Category::Security, "error", "Fichier .env versionné — secrets et configuration potentiellement exposés"),
        ("committed-secret-file", Category::Security, "warning", "Fichier de secrets/clé versionné — à retirer du dépôt et révoquer"),
        ("private-key-block", Category::Security, "error", "Clé privée en clair dans un fichier versionné — à retirer et révoquer"),
        ("multiple-lockfiles", Category::Git, "warning", "Plusieurs lockfiles versionnés — garde un seul gestionnaire de paquets"),
        ("build-output-committed", Category::Git, "warning", "Dossier de build versionné — ajoute-le au .gitignore"),
        ("dockerfile-latest-tag", Category::Devops, "warning", "Image de base non figée (FROM ...:latest) — épingle une version précise pour une build reproductible"),
        ("dockerfile-root-user", Category::Devops, "info", "Aucune instruction USER — le conteneur tourne root par défaut, ajoute un utilisateur dédié"),
        ("dockerfile-hardcoded-secret", Category::Devops, "error", "Secret en clair dans l'image (ENV/ARG) — passe-le en secret de build ou en variable d'environnement au run"),
        ("dockerfile-missing-healthcheck", Category::Devops, "info", "Aucune instruction HEALTHCHECK — l'orchestrateur ne peut pas détecter un conteneur bloqué"),
        ("workflow-unpinned-action", Category::Devops, "warning", "Action non épinglée sur un commit (uses: owner/repo@ref) — utilise le SHA complet, pas une branche/tag réécriptible"),
        ("workflow-write-all-permissions", Category::Devops, "error", "Permissions \"write-all\" — donne un accès en écriture bien plus large que nécessaire au token du job"),
        ("workflow-secret-in-run", Category::Devops, "error", "Secret potentiellement imprimé dans les logs du job (echo + secrets.*)"),
        ("workflow-pull-request-target", Category::Devops, "info", "Déclencheur pull_request_target — a accès aux secrets même depuis une PR d'un fork"),
    ];
    RAW.iter()
        .map(|(id, cat, severity, message)| BuiltinRule {
            id: (*id).to_string(),
            category: cat.name(),
            severity,
            message: (*message).to_string(),
            source: "// Règle Rust — pas de fichier .herald (analyse cross-fichiers ou routée par nom de fichier plutôt que par langage).\n",
        })
        .collect()
}

pub fn list() -> Vec<(String, String)> {
    let extra = extra_rules().into_iter().map(|r| (r.id, r.category.to_string()));

    let mut all: Vec<(String, String)> = dsl::rule_list().into_iter().chain(extra).collect();
    all.sort_by(|a, b| a.0.cmp(&b.0));
    all
}

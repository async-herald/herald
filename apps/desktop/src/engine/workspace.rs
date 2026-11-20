//! Copies locales des projets distants (dépôt Git, démo) : l'analyse se fait
//! toujours sur un dossier de la machine. Un projet distant est cloné dans le
//! dossier de l'app, puis mis à jour (`git pull`) avant chaque analyse.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Dossier des dépôts clonés, dans le dossier de données de l'app.
const CLONES_DIR: &str = "depots";

/// Dépôt d'exemple proposé dans « Nouveau projet ».
pub const DEMO_REPOSITORY: &str = "https://github.com/async-herald/demo-repo";

/// Nom du dépôt tiré d'une URL HTTPS (`https://hôte/org/depot(.git)`).
pub fn repository_name(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    if rest.contains(['@', ' ', '\\', '?', '#']) {
        return None;
    }
    let mut parts = rest.trim_end_matches('/').split('/');
    let host = parts.next()?;
    let segments: Vec<&str> = parts.collect();
    if !host.contains('.') || segments.len() < 2 {
        return None;
    }
    let name = segments.last()?.trim_end_matches(".git");
    let valid = !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    valid.then(|| name.to_owned())
}

/// Dossier de destination libre : `depots/<nom>`, puis `<nom>-2`, `<nom>-3`…
pub fn clone_destination(name: &str) -> Option<PathBuf> {
    let base = crate::paths::data_dir()?.join(CLONES_DIR);
    (1..1000)
        .map(|n| {
            if n == 1 {
                base.join(name)
            } else {
                base.join(format!("{name}-{n}"))
            }
        })
        .find(|path| !path.exists())
}

/// Avancement d'un clonage, lu dans la sortie de `git clone --progress`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CloneProgress {
    /// 0 à 100, toutes étapes confondues.
    pub percent: u8,
    /// Étape en cours, en français (« Réception des fichiers »).
    pub step: &'static str,
    /// Détail de Git pour la réception (« 12.40 MiB | 3.10 MiB/s »).
    pub detail: String,
}

/// Lit une ligne de progression de Git (forcé en anglais, voir `git()`) :
/// « Receiving objects:  45% (450/1000), 1.20 MiB | 2.00 MiB/s ».
pub fn parse_clone_progress(line: &str) -> Option<CloneProgress> {
    let line = line.trim().trim_start_matches("remote:").trim();
    // Étapes de Git et la part de la barre qu'elles occupent.
    const STEPS: [(&str, &str, u32, u32); 5] = [
        ("Enumerating objects", "Préparation sur le serveur", 0, 3),
        ("Counting objects", "Préparation sur le serveur", 3, 6),
        ("Compressing objects", "Compression sur le serveur", 6, 10),
        ("Receiving objects", "Réception des fichiers", 10, 90),
        ("Resolving deltas", "Reconstitution des fichiers", 90, 100),
    ];
    let (prefix, step, from, to) = STEPS.iter().find(|(prefix, ..)| line.starts_with(prefix))?;
    let rest = line[prefix.len()..].trim_start_matches(':').trim();
    let pct: u32 = rest.split('%').next()?.trim().parse().ok()?;
    let detail = if *prefix == "Receiving objects" {
        rest.split_once("),")
            .map(|(_, tail)| {
                tail.trim()
                    .trim_end_matches(", done.")
                    .trim_end_matches('.')
                    .to_owned()
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    Some(CloneProgress {
        percent: (from + (to - from) * pct.min(100) / 100) as u8,
        step,
        detail,
    })
}

/// Clone superficiel (`--depth 20` : assez d'historique pour les règles de commit).
/// `on_progress` reçoit l'avancement au fil de la sortie de Git.
pub fn git_clone(
    url: &str,
    dest: &Path,
    mut on_progress: impl FnMut(CloneProgress),
) -> Result<(), CloneError> {
    use std::io::Read;
    use std::process::Stdio;

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|_| CloneError::other("Impossible de créer le dossier des dépôts.", None))?;
    }
    let mut child = git()
        .args(["clone", "--depth", "20", "--progress", "--"])
        .arg(url)
        .arg(dest)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| CloneError::other(git_missing(), None))?;

    // Git réécrit sa ligne de progression avec « \r » : on découpe sur \r et \n.
    let mut stderr_text = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let mut buf = [0u8; 4096];
        let mut pending = String::new();
        while let Ok(n) = stderr.read(&mut buf) {
            if n == 0 {
                break;
            }
            pending.push_str(&String::from_utf8_lossy(&buf[..n]));
            while let Some(pos) = pending.find(['\r', '\n']) {
                let line: String = pending.drain(..=pos).collect();
                if let Some(progress) = parse_clone_progress(&line) {
                    on_progress(progress);
                } else if !line.trim().is_empty() {
                    stderr_text.push_str(line.trim_end());
                    stderr_text.push('\n');
                }
            }
        }
        stderr_text.push_str(&pending);
    }
    let status = child
        .wait()
        .map_err(|_| CloneError::other(git_missing(), None))?;
    if status.success() {
        return Ok(());
    }
    let _ = std::fs::remove_dir_all(dest);
    Err(CloneError::from_stderr(url, &stderr_text))
}

/// Cause d'un clonage raté, pour expliquer quoi faire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloneFailure {
    /// Dépôt privé (ou introuvable) et aucun identifiant enregistré pour ce site.
    Private,
    /// Des identifiants sont enregistrés mais refusés (jeton expiré, mot de passe).
    BadCredentials,
    /// Le site répond que le dépôt n'existe pas ou que ce compte n'y a pas accès.
    NoAccess,
    Network,
    Other,
}

/// Échec du clonage : cause, message et dernière ligne de Git (pour les cas imprévus).
#[derive(Clone, Debug)]
pub struct CloneError {
    pub failure: CloneFailure,
    pub message: String,
    pub detail: Option<String>,
    /// Commande à lancer dans le Terminal pour enregistrer ses identifiants.
    pub command: Option<String>,
}

impl CloneError {
    fn other(message: impl Into<String>, detail: Option<String>) -> Self {
        Self {
            failure: CloneFailure::Other,
            message: message.into(),
            detail,
            command: None,
        }
    }

    /// Lit la sortie de Git (forcée en anglais, voir `git()`).
    pub fn from_stderr(url: &str, stderr: &str) -> Self {
        let has = |needles: &[&str]| needles.iter().any(|n| stderr.contains(n));
        let host = url
            .strip_prefix("https://")
            .and_then(|rest| rest.split('/').next())
            .unwrap_or("le site");
        let failure = if has(&[
            "Authentication failed",
            "Invalid username or password",
            "Invalid username or token",
            "Password authentication is not supported",
            "HTTP Basic: Access denied",
        ]) {
            CloneFailure::BadCredentials
        } else if has(&[
            "could not read Username",
            "could not read Password",
            "terminal prompts disabled",
        ]) {
            CloneFailure::Private
        } else if has(&[
            "Repository not found",
            "not found",
            "returned error: 403",
            "returned error: 404",
            "Permission denied",
        ]) {
            CloneFailure::NoAccess
        } else if has(&[
            "Could not resolve host",
            "Failed to connect",
            "Connection timed out",
            "Operation timed out",
            "Connection refused",
            "SSL",
        ]) {
            CloneFailure::Network
        } else {
            CloneFailure::Other
        };
        let message = match failure {
            CloneFailure::Private => format!(
                "Ce dépôt est privé (ou l'adresse est fausse) et Git n'a aucun identifiant \
                 enregistré pour {host}."
            ),
            CloneFailure::BadCredentials => format!(
                "{host} refuse les identifiants enregistrés dans Git : jeton expiré ou mot de \
                 passe (GitHub et GitLab exigent un jeton d'accès)."
            ),
            CloneFailure::NoAccess => format!(
                "{host} indique que ce dépôt n'existe pas ou que votre compte n'y a pas accès. \
                 Vérifiez l'adresse et vos droits sur le dépôt."
            ),
            CloneFailure::Network => {
                format!("Impossible de joindre {host}. Vérifiez votre connexion.")
            }
            CloneFailure::Other => "Git n'a pas pu cloner ce dépôt.".to_owned(),
        };
        let command = matches!(
            failure,
            CloneFailure::Private | CloneFailure::BadCredentials | CloneFailure::NoAccess
        )
        .then(|| format!("git clone {url}"));
        let detail = stderr
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .last()
            .map(|line| line.trim_start_matches("fatal:").trim().to_owned());
        Self {
            failure,
            message,
            detail: (failure == CloneFailure::Other).then_some(detail).flatten(),
            command,
        }
    }

    /// Étapes pour donner accès à un dépôt privé ; vide si ce n'est pas le problème.
    pub fn help_steps(&self) -> Vec<String> {
        match self.failure {
            CloneFailure::Private | CloneFailure::NoAccess | CloneFailure::BadCredentials => {
                let mut steps = vec![
                    "Ouvrez le Terminal et lancez la commande ci-dessous une seule fois."
                        .to_owned(),
                    "Connectez-vous quand Git le demande. Pour GitHub et GitLab, le mot de passe \
                     est un jeton d'accès personnel (GitHub : Settings → Developer settings → \
                     Personal access tokens, droit « repo »)."
                        .to_owned(),
                    "Git garde vos identifiants dans le Trousseau : revenez ici et clonez à \
                     nouveau."
                        .to_owned(),
                ];
                if self.failure == CloneFailure::BadCredentials {
                    steps.insert(
                        0,
                        "Supprimez l'ancien identifiant : Trousseau d'accès, cherchez le nom du \
                         site (ex. github.com), supprimez l'entrée « mot de passe Internet »."
                            .to_owned(),
                    );
                }
                steps
            }
            CloneFailure::Network | CloneFailure::Other => Vec::new(),
        }
    }
}

/// Prépare le dossier à analyser : reclone un projet distant disparu, met à jour
/// une copie existante. Une mise à jour impossible (hors ligne) n'empêche pas
/// l'analyse de la copie locale.
pub fn prepare(path: &Path, remote: Option<&str>) -> Result<(), String> {
    match remote {
        Some(url) if !path.is_dir() => git_clone(url, path, |_| {}).map_err(|err| err.message),
        Some(_) => {
            let pulled = git()
                .arg("-C")
                .arg(path)
                .args(["pull", "--ff-only", "--quiet"])
                .output()
                .map_err(|_| git_missing())?;
            if !pulled.status.success() {
                eprintln!(
                    "herald: mise à jour de {} impossible, analyse de la copie locale ({})",
                    path.display(),
                    String::from_utf8_lossy(&pulled.stderr).trim()
                );
            }
            Ok(())
        }
        None if path.is_dir() => Ok(()),
        None => Err(format!("Dossier introuvable : {}", path.display())),
    }
}

/// Branche et sujets des 20 derniers commits, comme `git_context` du CLI Herald.
pub fn git_context(dir: &Path) -> Option<herald_engine::types::GitContext> {
    let run = |args: &[&str]| -> Option<String> {
        let out = git().arg("-C").arg(dir).args(args).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        (!text.is_empty()).then_some(text)
    };
    let branch = run(&["rev-parse", "--abbrev-ref", "HEAD"]);
    let commits: Vec<String> = run(&["log", "-20", "--format=%s"])
        .map(|text| text.lines().map(str::to_owned).collect())
        .unwrap_or_default();
    if branch.is_none() && commits.is_empty() {
        return None;
    }
    Some(herald_engine::types::GitContext { branch, commits })
}

fn git() -> Command {
    let mut command = Command::new("git");
    // Pas de demande d'identifiants : un dépôt privé échoue proprement.
    command.env("GIT_TERMINAL_PROMPT", "0");
    // Git Credential Manager ne doit pas ouvrir de fenêtre non plus.
    command.env("GCM_INTERACTIVE", "never");
    // Messages en anglais : `CloneError` les reconnaît quelle que soit la langue du Mac.
    command
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("LANGUAGE", "C");
    command
}

fn git_missing() -> String {
    "Git est introuvable. Installez les outils Xcode (xcode-select --install).".to_owned()
}

#[cfg(test)]
mod tests {
    use super::{CloneError, CloneFailure, parse_clone_progress, repository_name};

    #[test]
    fn clone_progress_is_read_from_git_output() {
        let received =
            parse_clone_progress("Receiving objects:  50% (500/1000), 1.20 MiB | 2.00 MiB/s")
                .unwrap();
        assert_eq!(received.percent, 50);
        assert_eq!(received.step, "Réception des fichiers");
        assert_eq!(received.detail, "1.20 MiB | 2.00 MiB/s");
        let counting =
            parse_clone_progress("remote: Counting objects: 100% (42/42), done.").unwrap();
        assert_eq!(counting.percent, 6);
        assert_eq!(
            parse_clone_progress("Resolving deltas: 100% (3/3), done.")
                .unwrap()
                .percent,
            100
        );
        assert!(parse_clone_progress("Cloning into 'x'...").is_none());
    }

    #[test]
    fn clone_errors_are_explained() {
        let url = "https://github.com/org/prive";
        let private = CloneError::from_stderr(
            url,
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled\n",
        );
        assert_eq!(private.failure, CloneFailure::Private);
        assert!(private.message.contains("github.com"));
        assert_eq!(
            private.command.as_deref(),
            Some("git clone https://github.com/org/prive")
        );
        assert!(!private.help_steps().is_empty());

        let refused = CloneError::from_stderr(
            url,
            "remote: Invalid username or token.\nfatal: Authentication failed for 'https://github.com/org/prive/'\n",
        );
        assert_eq!(refused.failure, CloneFailure::BadCredentials);
        assert_eq!(refused.help_steps().len(), 4);

        let missing = CloneError::from_stderr(
            url,
            "remote: Repository not found.\nfatal: repository 'https://github.com/org/prive/' not found\n",
        );
        assert_eq!(missing.failure, CloneFailure::NoAccess);

        let offline = CloneError::from_stderr(
            url,
            "fatal: unable to access 'https://github.com/org/prive/': Could not resolve host: github.com\n",
        );
        assert_eq!(offline.failure, CloneFailure::Network);
        assert!(offline.command.is_none());

        let odd = CloneError::from_stderr(url, "fatal: something unexpected\n");
        assert_eq!(odd.failure, CloneFailure::Other);
        assert_eq!(odd.detail.as_deref(), Some("something unexpected"));
    }

    #[test]
    fn repository_name_from_https_urls() {
        assert_eq!(
            repository_name("https://github.com/async-herald/herald-engine").as_deref(),
            Some("herald-engine")
        );
        assert_eq!(
            repository_name("https://gitlab.com/groupe/sous/projet.git/").as_deref(),
            Some("projet")
        );
        assert_eq!(repository_name("git@github.com:org/depot.git"), None);
        assert_eq!(repository_name("http://github.com/org/depot"), None);
        assert_eq!(repository_name("https://github.com/org"), None);
        assert_eq!(
            repository_name("https://user:pass@github.com/org/depot"),
            None
        );
        assert_eq!(repository_name("https://github.com/org/.."), None);
    }
}

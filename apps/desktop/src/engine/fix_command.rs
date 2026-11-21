//! Commande à taper pour corriger (ou mettre à jour) une dépendance, selon
//! l'outil du projet : celui du lockfile d'où vient la version, sinon celui
//! d'un lockfile du même dossier, sinon l'outil par défaut de l'écosystème.

use super::cve::Dependency;

/// Commande proposée dans l'onglet Dépendances.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixCommand {
    /// À coller dans un terminal, à la racine du projet.
    pub command: String,
    /// Précision utile (dépendance transitive, fichier à modifier…).
    pub note: Option<String>,
}

/// Outil de gestion des paquets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Npm,
    Yarn,
    Pnpm,
    Bun,
    Cargo,
    Pip,
    Poetry,
    Uv,
    Pipenv,
    Go,
    Composer,
}

/// Outil désigné par un fichier (lockfile ou manifeste).
fn tool_of_file(file: &str) -> Option<Tool> {
    Some(match file {
        "package-lock.json" | "npm-shrinkwrap.json" => Tool::Npm,
        "yarn.lock" => Tool::Yarn,
        "pnpm-lock.yaml" => Tool::Pnpm,
        "bun.lock" | "bun.lockb" => Tool::Bun,
        "Cargo.lock" | "Cargo.toml" => Tool::Cargo,
        "poetry.lock" => Tool::Poetry,
        "uv.lock" => Tool::Uv,
        "Pipfile" | "Pipfile.lock" => Tool::Pipenv,
        "go.mod" | "go.sum" => Tool::Go,
        "composer.json" | "composer.lock" => Tool::Composer,
        name if name.starts_with("requirements") && name.ends_with(".txt") => Tool::Pip,
        _ => return None,
    })
}

fn default_tool(ecosystem: &str) -> Option<Tool> {
    Some(match ecosystem {
        "npm" => Tool::Npm,
        "crates.io" => Tool::Cargo,
        "PyPI" => Tool::Pip,
        "Go" => Tool::Go,
        "Packagist" => Tool::Composer,
        _ => return None,
    })
}

/// Dossier et nom de fichier : `("api", "Cargo.toml")`, `("", "package.json")`.
fn split(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

/// Outil du paquet : son fichier source s'il est parlant (lockfile), sinon un
/// lockfile du même écosystème dans le même dossier, sinon l'outil par défaut.
fn tool_for(dep: &Dependency, all: &[Dependency]) -> Option<Tool> {
    let (dir, file) = split(&dep.source);
    let from_lockfile = |file: &str| tool_of_file(file).filter(|_| file != "package.json");
    // package.json ou pyproject.toml ne disent pas l'outil : on cherche à côté.
    if let Some(tool) = from_lockfile(file) {
        return Some(tool);
    }
    all.iter()
        .filter(|other| other.ecosystem == dep.ecosystem)
        .filter_map(|other| {
            let (other_dir, other_file) = split(&other.source);
            (other_dir == dir)
                .then(|| from_lockfile(other_file))
                .flatten()
        })
        .next()
        .or_else(|| default_tool(&dep.ecosystem))
}

/// Commande pour amener `dep` à `target` (version corrigée), ou pour le
/// mettre à jour si `target` est `None`. `all` : toutes les dépendances du
/// projet (pour reconnaître l'outil). `None` si l'écosystème est inconnu.
pub fn fix_command(
    dep: &Dependency,
    target: Option<&str>,
    all: &[Dependency],
) -> Option<FixCommand> {
    let tool = tool_for(dep, all)?;
    let name = dep.name.as_str();
    let direct = dep.direct;
    let (command, note): (String, Option<String>) = match (tool, target) {
        // ── JavaScript ──
        (Tool::Npm, Some(v)) if direct => (format!("npm install {name}@{v}"), None),
        (Tool::Npm, Some(v)) => (
            format!("npm update {name}"),
            Some(format!(
                "Dépendance transitive. Si {name} reste sous {v}, mettez à jour le paquet qui \
                 l'utilise, ou ajoutez \"overrides\": {{ \"{name}\": \"{v}\" }} dans package.json."
            )),
        ),
        (Tool::Npm, None) => (format!("npm update {name}"), None),
        (Tool::Yarn, Some(v)) if direct => (format!("yarn add {name}@{v}"), None),
        (Tool::Yarn, Some(v)) => (
            format!("yarn upgrade {name}"),
            Some(format!(
                "Dépendance transitive. Si {name} reste sous {v}, ajoutez \"resolutions\": \
                 {{ \"{name}\": \"{v}\" }} dans package.json."
            )),
        ),
        (Tool::Yarn, None) => (format!("yarn upgrade {name}"), None),
        (Tool::Pnpm, Some(v)) if direct => (format!("pnpm add {name}@{v}"), None),
        (Tool::Pnpm, Some(v)) => (
            format!("pnpm update {name} --depth Infinity"),
            Some(format!(
                "Dépendance transitive. Si {name} reste sous {v}, ajoutez \"pnpm\": \
                 {{ \"overrides\": {{ \"{name}\": \"{v}\" }} }} dans package.json."
            )),
        ),
        (Tool::Pnpm, None) => (format!("pnpm update {name}"), None),
        (Tool::Bun, Some(v)) => (format!("bun add {name}@{v}"), None),
        (Tool::Bun, None) => (format!("bun update {name}"), None),
        // ── Rust ──
        (Tool::Cargo, Some(v)) => (
            format!("cargo update -p {name} --precise {v}"),
            direct.then(|| {
                format!(
                    "Si la version demandée dans Cargo.toml l'empêche, passez-la à \"{v}\" \
                     puis relancez la commande."
                )
            }),
        ),
        (Tool::Cargo, None) => (format!("cargo update -p {name}"), None),
        // ── Python ──
        (Tool::Pip, Some(v)) => (
            format!("pip install --upgrade \"{name}>={v}\""),
            Some(format!(
                "Reportez ensuite {name}>={v} dans votre fichier requirements."
            )),
        ),
        (Tool::Pip, None) => (format!("pip install --upgrade {name}"), None),
        (Tool::Poetry, Some(v)) if direct => (format!("poetry add \"{name}>={v}\""), None),
        (Tool::Poetry, _) => (format!("poetry update {name}"), None),
        (Tool::Uv, Some(v)) if direct => (format!("uv add \"{name}>={v}\""), None),
        (Tool::Uv, _) => (format!("uv lock --upgrade-package {name}"), None),
        (Tool::Pipenv, _) => (format!("pipenv update {name}"), None),
        // ── Go ──
        (Tool::Go, Some(v)) => {
            let v = if v.starts_with('v') {
                v.to_owned()
            } else {
                format!("v{v}")
            };
            (format!("go get {name}@{v} && go mod tidy"), None)
        }
        (Tool::Go, None) => (format!("go get -u {name} && go mod tidy"), None),
        // ── PHP ──
        (Tool::Composer, Some(v)) if direct => (format!("composer require {name}:^{v}"), None),
        (Tool::Composer, _) => (format!("composer update {name} --with-dependencies"), None),
    };
    // Le manifeste est dans un sous-dossier : on y va d'abord.
    let (dir, _) = split(&dep.source);
    let command = if dir.is_empty() {
        command
    } else {
        format!("cd {dir} && {command}")
    };
    Some(FixCommand { command, note })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dep(ecosystem: &str, name: &str, direct: bool, source: &str) -> Dependency {
        Dependency {
            ecosystem: ecosystem.into(),
            name: name.into(),
            version: "1.0.0".into(),
            direct,
            resolved: true,
            source: source.into(),
        }
    }

    fn command(d: &Dependency, target: Option<&str>, all: &[Dependency]) -> String {
        fix_command(d, target, all).unwrap().command
    }

    #[test]
    fn npm_direct_and_transitive() {
        let vite = dep("npm", "vite", true, "package-lock.json");
        let brace = dep("npm", "brace-expansion", false, "package-lock.json");
        let all = [vite.clone(), brace.clone()];
        assert_eq!(
            command(&vite, Some("6.4.4"), &all),
            "npm install vite@6.4.4"
        );
        let fix = fix_command(&brace, Some("5.0.10"), &all).unwrap();
        assert_eq!(fix.command, "npm update brace-expansion");
        assert!(fix.note.unwrap().contains("\"overrides\""));
        assert_eq!(command(&vite, None, &all), "npm update vite");
    }

    #[test]
    fn the_tool_comes_from_a_lockfile_next_to_the_manifest() {
        // Version lue dans package.json, mais un yarn.lock est à côté.
        let react = dep("npm", "react", true, "web/package.json");
        let other = dep("npm", "lodash", false, "web/yarn.lock");
        let all = [react.clone(), other];
        assert_eq!(
            command(&react, Some("18.3.1"), &all),
            "cd web && yarn add react@18.3.1"
        );
        // Sans lockfile : npm par défaut.
        assert_eq!(
            command(&react, None, std::slice::from_ref(&react)),
            "cd web && npm update react"
        );
    }

    #[test]
    fn other_ecosystems() {
        let serde = dep("crates.io", "serde", true, "Cargo.lock");
        assert_eq!(
            command(&serde, Some("1.0.200"), std::slice::from_ref(&serde)),
            "cargo update -p serde --precise 1.0.200"
        );
        let requests = dep("PyPI", "requests", true, "requirements.txt");
        assert_eq!(
            command(&requests, Some("2.32.0"), std::slice::from_ref(&requests)),
            "pip install --upgrade \"requests>=2.32.0\""
        );
        let poetry = dep("PyPI", "django", true, "poetry.lock");
        assert_eq!(
            command(&poetry, Some("5.0.1"), std::slice::from_ref(&poetry)),
            "poetry add \"django>=5.0.1\""
        );
        let uv = dep("PyPI", "jinja2", false, "uv.lock");
        assert_eq!(
            command(&uv, Some("3.1.4"), std::slice::from_ref(&uv)),
            "uv lock --upgrade-package jinja2"
        );
        let net = dep("Go", "golang.org/x/net", true, "go.mod");
        assert_eq!(
            command(&net, Some("0.23.0"), std::slice::from_ref(&net)),
            "go get golang.org/x/net@v0.23.0 && go mod tidy"
        );
        let guzzle = dep("Packagist", "guzzlehttp/guzzle", true, "composer.lock");
        assert_eq!(
            command(&guzzle, Some("7.4.5"), std::slice::from_ref(&guzzle)),
            "composer require guzzlehttp/guzzle:^7.4.5"
        );
        assert!(fix_command(&dep("Maven", "x", true, "pom.xml"), None, &[]).is_none());
    }
}

fn main() {
    // Les règles de `rules/` sont embarquées par `include_dir!`, qui ne déclare
    // pas ses fichiers à Cargo : sans cette ligne, modifier une règle ne
    // recompile pas.
    println!("cargo:rerun-if-changed=rules");

    // Exécutable Windows : icône et informations de version.
    println!("cargo:rerun-if-changed=assets/windows");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_resources();
    }
}

/// Remplit le modèle `assets/windows/herald.rc` (version, chemin absolu de
/// l'icône) dans `OUT_DIR`, puis le compile dans l'exécutable.
fn embed_windows_resources() {
    let env = |name: &str| std::env::var(name).unwrap_or_else(|_| "0".to_owned());
    let manifest_dir = std::path::PathBuf::from(env("CARGO_MANIFEST_DIR"));
    let template = std::fs::read_to_string(manifest_dir.join("assets/windows/herald.rc"))
        .expect("modèle assets/windows/herald.rc");
    // Barres obliques : rc.exe les accepte et elles n'ont pas à être échappées.
    let icon = manifest_dir
        .join("assets/windows/herald.ico")
        .to_string_lossy()
        .replace('\\', "/");
    let rc = template
        .replace("@ICON@", &icon)
        .replace("@VERSION@", &env("CARGO_PKG_VERSION"))
        .replace("@MAJOR@", &env("CARGO_PKG_VERSION_MAJOR"))
        .replace("@MINOR@", &env("CARGO_PKG_VERSION_MINOR"))
        .replace("@PATCH@", &env("CARGO_PKG_VERSION_PATCH"));
    let out = std::path::PathBuf::from(env("OUT_DIR")).join("herald.rc");
    std::fs::write(&out, rc).expect("écriture de herald.rc");
    embed_resource::compile(&out, embed_resource::NONE)
        .manifest_optional()
        .expect("ressources Windows (icône, version)");
}

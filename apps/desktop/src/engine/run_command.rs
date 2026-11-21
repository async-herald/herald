//! Exécution d'une commande de correction (onglet Dépendances) dans le dossier
//! du projet.
//!
//! Une app ouverte depuis le Dock ou le Finder n'a pas le PATH du terminal :
//! `npm`, `cargo` ou `go` y sont introuvables. La commande passe donc par le
//! shell de connexion de l'utilisateur en mode interactif (`$SHELL -lic`), qui
//! charge sa configuration (Homebrew, nvm, rustup…). Sous Windows : `cmd /C`.

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

/// Sortie gardée (les derniers caractères), pour ne pas grossir sans fin.
const MAX_OUTPUT: usize = 20_000;

/// Ajoute une ligne à la sortie partagée, en ne gardant que la fin.
pub fn push_line(output: &Mutex<String>, line: &str) {
    if let Ok(mut text) = output.lock() {
        text.push_str(line);
        text.push('\n');
        if text.len() > MAX_OUTPUT {
            let mut cut = text.len() - MAX_OUTPUT;
            while !text.is_char_boundary(cut) {
                cut += 1;
            }
            text.drain(..cut);
        }
    }
}

fn shell_command(command: &str) -> Command {
    if cfg!(windows) {
        let mut shell = Command::new("cmd");
        shell.args(["/C", command]);
        shell
    } else {
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|shell| !shell.is_empty())
            .unwrap_or_else(|| "/bin/zsh".to_owned());
        let mut process = Command::new(shell);
        process.args(["-lic", command]);
        process
    }
}

/// Lance `command` dans `dir` et recopie stdout et stderr, ligne par ligne,
/// dans `output` au fil de l'eau. Bloquant : à appeler hors du fil de
/// l'interface. `Ok(true)` si la commande a réussi (code 0).
pub fn run(dir: &Path, command: &str, output: Arc<Mutex<String>>) -> Result<bool, String> {
    push_line(&output, &format!("$ {command}"));
    let mut child = shell_command(command)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("Impossible de lancer le shell : {err}"))?;

    let pipe = |stream: Option<Box<dyn Read + Send>>, output: Arc<Mutex<String>>| {
        std::thread::spawn(move || {
            if let Some(stream) = stream {
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    push_line(&output, &line);
                }
            }
        })
    };
    let stdout = pipe(
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
        Arc::clone(&output),
    );
    let stderr = pipe(
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
        Arc::clone(&output),
    );
    let status = child
        .wait()
        .map_err(|err| format!("La commande s'est interrompue : {err}"))?;
    let _ = stdout.join();
    let _ = stderr.join();
    Ok(status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_keeps_only_the_end() {
        let output = Mutex::new(String::new());
        for _ in 0..3_000 {
            push_line(&output, "0123456789");
        }
        let text = output.lock().unwrap();
        assert!(text.len() <= MAX_OUTPUT);
        assert!(text.ends_with("0123456789\n"));
    }

    #[cfg(unix)]
    #[test]
    fn runs_in_the_project_folder_and_reports_failure() {
        let dir = std::env::temp_dir();
        let output = Arc::new(Mutex::new(String::new()));
        let fine = run(&dir, "echo herald-ok", Arc::clone(&output)).unwrap();
        assert!(fine);
        assert!(output.lock().unwrap().contains("herald-ok"));
        let failed = run(&dir, "exit 3", Arc::clone(&output)).unwrap();
        assert!(!failed);
    }
}

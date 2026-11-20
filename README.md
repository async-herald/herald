# Herald — analyse de code déterministe, sans IA

**Herald applique des règles explicites à votre code et renvoie des problèmes précis et une note sur 100.**
Pas de modèle, pas de boîte noire : chaque problème vient d'une règle lisible, testée par ses exemples.

Ce dépôt contient tout Herald côté machine de l'utilisateur :

| Dossier | Contenu |
|---|---|
| [`crates/herald-engine`](crates/herald-engine) | Le moteur : parsers (tree-sitter, 15 langages), langage de règles `.herald`, analyseurs intégrés, notation. 117 règles. |
| [`crates/herald-cli`](crates/herald-cli) | Le CLI `herald`, pour analyser un dossier en local, dans un terminal ou en CI. |
| [`apps/desktop`](apps/desktop) | **Async Herald**, l'app de bureau native (Rust, [GPUI](https://gpui.rs)) pour macOS, Windows et Linux. |

Toute l'analyse se fait sur votre machine : aucun fichier source n'est envoyé.
Le service [herald.codes](https://herald.codes) ajoute le compte, l'historique des notes et l'analyse de dépôts
depuis le navigateur ; il utilise le même moteur.

## App de bureau

- **macOS** : `brew install async-herald/tap/herald`, ou le `.dmg` depuis [herald.codes](https://herald.codes).
- **Linux** : `curl -fsSL https://herald.codes/install.sh | sh` (AppImage ou `.deb`).
- **Windows** : installeur depuis [herald.codes](https://herald.codes).

Compiler et lancer depuis les sources (macOS : Xcode requis pour les shaders Metal de GPUI) :

```bash
cargo run -p async-herald-desktop --release
```

Détails (architecture, publication, mises à jour) : [`apps/desktop/README.md`](apps/desktop/README.md).

## CLI

```bash
git clone https://github.com/async-herald/herald
cd herald
cargo build --release -p herald-cli

./target/release/herald                 # analyse le dossier courant
./target/release/herald src/            # un dossier
./target/release/herald app.js          # un fichier
./target/release/herald . --json        # sortie JSON
```

Le CLI renvoie un code de sortie ≠ 0 s'il trouve des erreurs, pratique en CI.

## Écrire une règle

Les règles sont de simples fichiers texte `.herald` (lisibles, testables). Voir
[`crates/herald-engine/src/dsl/README.md`](crates/herald-engine/src/dsl/README.md)
et [CONTRIBUTING.md](CONTRIBUTING.md). Les contributions de règles sont bienvenues.

## Développement

```bash
cargo fmt -p async-herald-desktop --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

## Licence

**AGPL-3.0-only** — voir [LICENSE](LICENSE). Vous pouvez utiliser, modifier et redistribuer Herald ; si vous le
proposez comme service en réseau, vous devez en publier le code source (y compris vos modifications).

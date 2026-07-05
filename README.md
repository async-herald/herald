# Herald — moteur d'analyse de code (open source)

**Analyse de code déterministe, sans IA, sans stockage.** Herald applique des
règles explicites (pas de modèle, pas de boîte noire) à votre code et renvoie
des problèmes précis et un score.

Ce dépôt est le **cœur open source** de Herald :

- 🔧 **`herald-engine`** — le moteur : parsers (tree-sitter, 15 langages), le
  langage de règles `.herald`, les analyseurs built-in, le scoring.
- 📏 **78 règles** gratuites (sécurité, architecture, qualité, fiabilité…).
- 💻 **`herald-cli`** — un CLI pour analyser vos fichiers **en local**.

> Le service hébergé **[herald.codes](https://herald.codes)** ajoute ce que le
> CLI local ne fait pas : analyse depuis le navigateur, **dépôts privés** liés
> (GitHub/GitLab) sans clone, **historique & rapports**, **équipes**, **CI/CD**
> (analyse de chaque PR), gros dépôts, et des **catégories de règles premium**.
> Ce dépôt ne contient pas la plateforme ni les règles premium.

## Installation

```bash
git clone https://github.com/async-herald/herald
cd herald
cargo build --release
```

## Utilisation

```bash
./target/release/herald                 # analyse le dossier courant
./target/release/herald src/            # un dossier
./target/release/herald app.js          # un fichier
./target/release/herald . --json        # sortie JSON
```

Le CLI renvoie un code de sortie ≠ 0 s'il trouve des erreurs — pratique en CI.

## Écrire une règle

Les règles sont de simples fichiers texte `.herald` (lisibles, testables). Voir
[`crates/herald-engine/src/dsl/README.md`](crates/herald-engine/src/dsl/README.md)
et [CONTRIBUTING.md](CONTRIBUTING.md). Les contributions de règles sont
bienvenues.

## Licence

**AGPL-3.0-only** — voir [LICENSE](LICENSE). Vous pouvez utiliser, modifier et
redistribuer le moteur ; si vous le proposez comme service en réseau, vous devez
en publier le code source (y compris vos modifications).

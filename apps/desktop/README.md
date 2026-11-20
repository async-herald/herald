# Async Herald Desktop

Application bureau native de [Herald](https://herald.codes), écrite en Rust avec [GPUI](https://gpui.rs)
(le framework d'interface de Zed). L'analyse est faite sur la machine de l'utilisateur : aucun fichier source n'est
envoyé sur le réseau.

L'analyse est faite en local avec `herald-engine` (`crates/herald-engine` de ce dépôt), le même moteur que le
service herald.codes. Les alertes CVE viennent de la base OSV et les analyses sont sauvegardées en local (SQLite).
L'app s'ouvre directement sur les projets : pas de compte, aucune donnée envoyée hors des requêtes OSV.

L'app est un membre du workspace Cargo du dépôt : le moteur est pris dans `crates/herald-engine`, et les binaires
sont produits dans `target/` à la racine du dépôt.

## Lancer

```bash
cargo run -p async-herald-desktop               # debug
cargo run -p async-herald-desktop --release     # optimisé
cargo test -p async-herald-desktop              # tests
```

Prérequis macOS : Xcode (compilation des shaders Metal de GPUI). Rust 1.85+.

### Application macOS (icône dans le Dock)

`cargo run` lance un exécutable sans bundle : macOS lui donne l'icône générique. Pour l'icône Herald dans le Dock,
le Finder et ⌘⇥ :

```bash
scripts/bundle-macos.sh                         # → ../../target/release/bundle/Herald.app
open "../../target/release/bundle/Herald.app"
```

Le script compile la release, génère `Herald.icns` depuis `assets/brand/angel.png` et écrit l'`Info.plist`
(identifiant `fr.itsasync.herald.desktop`, le même que le Trousseau). Sans `HERALD_SIGN_ID`, le bundle n'est ni signé
ni notarisé.

## Écrans

Les écrans partagent la même barre latérale (navigation, projets). Un dossier peut être déposé
n'importe où dans la fenêtre.

- **Vue d'ensemble** : prise en main (avec l'engagement « sans IA »), note du projet et détail par catégorie,
  gains rapides.
- **Problèmes** : synthèse de la note, liste filtrable par catégorie, fiche du problème sélectionné (explication,
  extrait de code autour de la ligne, correctif proposé).
- **Nouveau projet** : le bouton + ouvre une modale avec trois types : dossier local (sélecteur natif), dépôt Git
  (URL HTTPS publique, clonée avec `git clone --depth 1` dans `~/Library/Application Support/fr.itsasync.herald.desktop/depots`)
  et projet de démo.
- **Lancer / relancer une analyse** : animation plein écran reprise de herald.codes (éclat, rayons, l'ange qui grandit,
  pourcentage plafonné à 92 % jusqu'au résultat, étape en cours). L'analyse est faite par `src/engine`, voir plus bas.
- **Règles** : catalogue des règles officielles appliquées à chaque analyse, filtrable par catégorie, paginé
  (20 par page) ; un clic sur une règle affiche son code `.herald`.
- **Projets** : survoler un projet affiche une croix pour le retirer de l'app, après confirmation. Le dossier sur le
  disque n'est jamais modifié.
- **Motivation à corriger** : chaque problème affiche les points qu'il rapporte (pénalités et poids de
  `AsyncHerald/src/herald/scoring.rs`), « Marquer comme corrigé » met à jour le score projeté et le prochain palier,
  un bandeau félicite quand la note monte, l'accueil liste les gains rapides.
- **Alertes CVE** (`src/engine/cve.rs`, `src/engine/lockfile.rs`) : versions **installées**, directes et
  transitives, lues dans les lockfiles (`package-lock.json`, `npm-shrinkwrap.json`, `yarn.lock`, `pnpm-lock.yaml`,
  `Cargo.lock`, `poetry.lock`, `uv.lock`, `Pipfile.lock`, `composer.lock`, `go.mod`) ; sans lockfile, versions
  déclarées dans les manifestes (`package.json`, `Cargo.toml`, `requirements.txt`). Une contrainte qui ne dit rien
  de la version installée (`>=2.0`, `*`) n'est pas vérifiée ; une borne basse (`^4.17.15`, `~=1.4`) l'est, mais la
  faille est marquée « à confirmer » et ne compte pas dans les alertes non lues. Croisement avec la base OSV
  (`/v1/querybatch`, puis fiche de chaque vulnérabilité, en cache 24 h ; avis retirés ignorés). CVE affiché quand il existe, CVSS 3 calculé depuis le vecteur, plus petite version corrigée au-dessus
  de la version installée. Contrôle après chaque analyse et à chaque lancement (une CVE publiée depuis apparaît sans
  relancer). Seuls les noms et versions des paquets sont envoyés à osv.dev.
- **Panneau de la note** : à droite des quatre onglets d'un projet (Plan, Revue, Code, Dépendances) ; note actuelle
  et estimée, relance de l'analyse, export du rapport (`src/export.rs`) en **PDF** (police Inter embarquée) ou en
  **Markdown** : note, catégories, problèmes par règle (explication, correctif, emplacements), dépendances
  vulnérables. Enregistré où l'utilisateur le choisit, puis montré dans le Finder.
- **Code** (`ui/ide.rs`) : explorateur virtualisé (seules les lignes visibles sont construites, lignes et compteurs
  en cache), dossiers à enfant unique regroupés (`src/main/java`), guides d'indentation, pastille de gravité et
  nombre de problèmes par fichier, filtre « Tous / Problèmes », « Replier », fichier actif mis en vue. L'éditeur
  met à jour l'index des lignes sur place à chaque frappe.
- **Dépendances** (`ui/dependencies.rs`, 4ᵉ onglet du projet après Plan, Revue et Code) : chaque paquet avec sa
  version, directe ou transitive, son écosystème, ses vulnérabilités (gravité la plus haute) et la version qui les
  corrige toutes. Filtres Vulnérables / Directes / Toutes, 50 paquets par page, clic sur un paquet vulnérable pour
  voir ses avis (CVE, CVSS, correctif, lien). « Revérifier auprès d'OSV » refait le contrôle sans relancer
  l'analyse ni décompter de quota. `HERALD_DEV_OPEN=deps` ouvre l'onglet après `HERALD_DEV_ANALYZE`.
- **Ressources** (`ui/resource_monitor.rs`) : pendant une analyse, barre en bas de la fenêtre avec la mémoire de
  Herald, sa courbe, le pic, la part de l'analyse et le processeur ; bilan (pic, durée) à la fin.

## Stockage local

`src/store.rs`, SQLite dans `~/Library/Application Support/fr.itsasync.herald.desktop/herald.db` : projets (ordre de
la barre latérale), historique des analyses (30 par projet) avec les corrections marquées, dépendances, alertes CVE
et leur état « lue », cache des fiches OSV. Le code source n'est pas copié : les extraits sont relus depuis le dossier
du projet à l'ouverture. Retirer un projet efface ses analyses et alertes.

## Analyse

`src/engine` reproduit l'analyse du service herald.codes, sur la machine :

1. **Copie locale** (`workspace.rs`) : un projet distant (dépôt Git, démo) est cloné dans
   `~/Library/Application Support/fr.itsasync.herald.desktop/depots`, puis mis à jour (`git pull --ff-only`) avant
   chaque analyse ; hors ligne, la copie existante est analysée.
2. **Fichiers** (`collect.rs`) : mêmes filtres que le backend (`source/filter.rs`) — dossiers ignorés, 500 Ko par
   fichier, bruit écarté, 5 000 fichiers au plus — et `herald.config.json` du projet appliqué.
3. **Règles** : catalogue intégré au moteur. Les règles de `rules/`
   (`engine/local_rules.rs`) remplacent celles de même identifiant : ce sont les règles de sécurité corrigées
   après la mesure de précision (voir « Précision des règles »).
4. **Moteur** : `herald_engine::analyze` avec le contexte Git local (branche, 20 derniers commits).
5. **Résultat** : regroupé comme `grouped_response` (7 catégories, note globale sur les catégories applicables,
   toutes détaillées). Les points d'une correction sont recalculés avec le
   barème du moteur.

Vérifié identique au backend (note globale et chaque catégorie) sur `async-herald/demo-repo` (5/100) et
`async-herald/herald-engine` (83/100). Aucun fichier n'est envoyé ; comme sur le backend, les noms et versions des
dépendances sont vérifiés auprès d'OSV et des registres de paquets.

## Précision des règles

Les règles de sécurité sont mesurées sur 60 dépôts open source connus (`bench/`, méthode dans `bench/README.md`) :
chaque relevé d'un échantillon est étiqueté vrai ou faux positif à la main.

- **Règles corrigées** (`rules/security/*.herald`) : chaque faux positif trouvé est devenu un cas `pass` de la
  règle ; `cargo test` rejoue tous les cas (`local_rules::tests`).
- **Hors surface d'attaque** : les relevés de sécurité dans la documentation, les exemples, les benchmarks, les tests,
  les fixtures et le code minifié sont écartés, sauf `leaked-secret` (`local_rules::dismissed`).
- **À revoir** : une règle sous 90 % de vrais positifs (`rules/precision.json`) reste affichée, avec la mention
  « À revoir » et sa précision mesurée, mais ne compte pas dans la note et ne rapporte pas de points.
- Mesurer à nouveau : `cargo run --release --example precision -- <corpus> <relevés.jsonl>`.

## Design

Les tailles et couleurs de l'interface viennent uniquement de `src/theme.rs` (`Theme`, `metrics`, `fonts`).

## Structure

```
src/
├── main.rs              # Application GPUI, menu, fenêtre
├── assets.rs            # Logo, icônes et polices embarqués (include_bytes!)
├── service.rs           # Services en ligne (version locale : aucun)
├── time.rs              # Libellés de date relatifs
├── store.rs             # Stockage local SQLite (projets, analyses, alertes, cache OSV)
├── engine/              # Analyse locale (herald-engine), dépôts clonés, CVE (cve.rs)
├── theme.rs             # Couleurs, métriques et polices de la charte
├── model/               # Types métier, indépendants de l'UI
│   ├── advisory.rs      # Alerte CVE
│   ├── grade.rs         # Note A+…F (même barème que AsyncHerald/scoring.rs)
│   ├── issue.rs         # Issue, Severity, Category
│   ├── project.rs       # Project
│   ├── report.rs        # AnalysisReport, corrections, score projeté, extraits de code
│   ├── sample.rs        # Dossiers proposés au premier lancement
│   └── usage.rs         # Quota d'analyses du jour
└── ui/
    ├── app.rs           # HeraldApp : vue racine, état de navigation et de sélection
    ├── sidebar.rs       # Barre latérale commune (projets, alertes, quota)
    ├── home.rs          # Vue d'ensemble, prise en main, helpers de mise en page
    ├── analysis.rs      # Écran Problèmes
    ├── alerts.rs        # Écran des alertes CVE
    ├── local.rs         # Compte, quota, mises à jour : version locale (sans service)
    ├── resource_monitor.rs # Mémoire et processeur pendant une analyse
    └── components/      # Composants sans état (RenderOnce)
        ├── button.rs    # Primary / Default / Ghost
        ├── icon.rs
        ├── labels.rs    # GradeText, SeverityLabel, CountBadge
        ├── logo.rs
        ├── progress_bar.rs
        ├── herald_emblem.rs # Emblème animé (flottement, ailes, halo, regard)
        └── text_input.rs  # Champ de saisie (adapté de l'exemple GPUI, Apache-2.0)
assets/
├── brand/angel.png      # L'ange Herald (depuis heraldapp-rs/public)
├── emblem/              # Calques de l'emblème animé (herald-pro-animated.svg)
├── icons/*.svg
└── fonts/               # Inter, Space Grotesk, JetBrains Mono (OFL, voir OFL.md)
```

## Prochaines étapes

1. Export SARIF (pour GitHub Code Scanning).
2. Afficher l'historique des notes d'un projet (`store::history`, déjà sauvegardé).

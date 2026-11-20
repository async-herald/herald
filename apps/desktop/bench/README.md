# Mesure de précision des règles de sécurité

Herald Desktop mesure la précision de chaque règle de sécurité sur un corpus de dépôts open source connus. Une
règle sous 90 % de vrais positifs passe « À revoir » : ses relevés restent visibles mais ne comptent pas dans la
note (`src/engine/local_rules.rs`, `rules/precision.json`).

## Corpus

60 dépôts clonés en `--depth 1` (`corpus.txt`) : frameworks et applications JS/TS, Python, Go, Rust, PHP, Java.
Ghost est cloné mais non mesuré : le moteur dépasse 3 minutes dessus (voir « Limites »).

## Méthode

1. **Relevés.** `cargo run --release --example precision -- <corpus> <relevés.jsonl>` analyse chaque dépôt comme
   l'app : même collecte de fichiers, règles de l'app (`rules/`) à la place de celles du moteur, filtre hors
   production. Manifestes écartés (aucune requête aux registres). `HERALD_PRECISION_BASELINE=1` mesure les règles
   du moteur seules, sans filtre.
2. **Échantillon.** Par règle, tirage déterministe (graine = nom de la règle) qui prend un relevé par dépôt à tour
   de rôle, pour qu'un gros dépôt ne domine pas : 10 relevés, 20 quand les 10 premiers sont tous vrais.
3. **Étiquetage.** Chaque relevé est lu avec son contexte et marqué :
   - **vrai** : risque de sécurité réel dans du code de production, sur lequel un développeur doit agir ;
   - **faux** : tout le reste (valeur constante ou interne, API utilisée de façon sûre, hash sans rôle de
     sécurité, documentation, exemple, test, code généré).
4. **Mesure.** `python3 -I bench/label.py <relevés.jsonl> <verdicts.json> <étiquettes.jsonl> rules/precision.json`
   rejoue le tirage, écrit une ligne par relevé étiqueté (dépôt, commit, fichier, ligne, verdict) et met à jour
   `rules/precision.json`.
5. **Correction.** Chaque faux positif devient un cas `pass` dans la règle de l'app (`rules/security/*.herald`,
   bloc `tests`), réécrit en quelques lignes plutôt que copié. `cargo test` rejoue tous les cas
   (`every_local_rule_passes_its_examples`).

## Fichiers

| Fichier | Contenu |
|---|---|
| `corpus.txt` | Dépôts du corpus |
| `verdicts-baseline.json`, `labels/baseline.jsonl` | Règles du moteur, sans filtre |
| `verdicts-after.json`, `labels/after.jsonl` | Règles de l'app, filtre hors production |
| `../rules/precision.json` | Mesure retenue pour chaque règle (celle qui s'applique dans l'app) |

## Limites

- Échantillons de 10 à 20 relevés : une règle à 10/10 a une précision probablement haute, pas prouvée à 90 %.
  Une règle sans relevé sur le corpus n'est pas mesurée et reste active.
- Le corpus est fait de bibliothèques et d'applications bien maintenues : il mesure surtout les faux positifs.
  Le rappel (failles manquées) demande un autre corpus (applications volontairement vulnérables).
- Les règles écrites en Rust dans le moteur (`taint-*`, `committed-*-file`, `private-key-block`) ne peuvent pas
  être corrigées depuis l'app : elles passent « À revoir » quand elles sont sous 90 %.

# Contribuer

Merci de contribuer à Herald ! Le moyen le plus utile de contribuer : **ajouter
des règles** `.herald`.

## Ajouter une règle

1. Crée un fichier dans `crates/herald-engine/rules/<catégorie>/<id>.herald`.
   Catégories du dépôt open source : `security`, `architecture`, `codesmell`,
   `reliability`. (Les catégories `ai`, `performance`, `testing`, `pro-pattern`,
   `git` sont réservées au produit hébergé.)
2. Décris la **forme** du code à détecter et affine avec des conditions `when` /
   `where`. La grammaire complète est dans
   [`crates/herald-engine/src/dsl/README.md`](crates/herald-engine/src/dsl/README.md).
3. Ajoute des **tests inline** (`tests { … => fail | pass }`) : c'est la preuve
   « zéro faux positif ». Exemple :

   ```
   rule no_eval {
     meta { type: security  severity: error
       message: "Évite eval() : exécution de code arbitraire" }
     match { js, ts: eval($X)   py: eval($X) }
     tests {
       js { "eval(userInput)" => fail
            "JSON.parse(userInput)" => pass }
     }
   }
   ```

## Vérifier

```bash
cargo build
cargo test           # exécute la suite du moteur
./target/debug/herald examples/   # essaie ta règle sur du code
```

Une règle n'est acceptée que si **tous ses exemples passent** et qu'elle ne
génère pas de faux positifs sur du code réel. La qualité prime sur la quantité.

## Style

- Français pour les messages destinés aux utilisateurs, anglais pour les
  identifiants de code.
- Une règle = un problème clair, un message qui explique *quoi* et *comment
  corriger*.

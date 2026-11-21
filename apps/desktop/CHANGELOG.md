# Nouveautés d'Async Herald

Chaque version a sa section « ## <version> » : elle est publiée telle quelle dans
« Mises à jour » de herald.codes (affichée par l'app) et annoncée sur Discord.
Une version sans section ne peut pas être publiée.

## 0.2.0
- Nouvel onglet Général, ouvert en premier : l'annonce du héraut (la note et son évolution, ce qui reste critique), puis quatre axes en grandes cartes, Sécurité, Qualité, Architecture (Pattern, Versioning, DevOps et Tests) et Dépendances, chacun avec sa note, ses problèmes, sa règle la plus fréquente ou ses alertes CVE, et la courbe de ses dernières analyses. Un clic sur une carte ouvre l'onglet de la catégorie.
- En-tête de projet unique sur tous les onglets (nom, chemin, dernière analyse) et onglets soulignés. La carte du code passe dans l'onglet Code (bascule Éditeur / Carte du code).
- Les grands nombres sont séparés par milliers (« 19 357 problèmes »).
- Nouvel onglet Patterns dans chaque projet. Chaque pattern de conception (responsabilité unique, DRY, MVC, couplage, frontières de modules, état partagé, injection de dépendances) a sa carte avec une icône et son état. Un clic ouvre sa page, avec un schéma « à éviter / à faire », des astuces et tous les endroits de votre code à revoir.
- L'onglet Revue est retiré. « Ouvrir » dans le Plan ouvre maintenant le problème directement dans l'onglet Code, à la bonne ligne.
- L'analyse et l'onglet Code ignorent les fichiers qui ne sont pas du code du projet : ceux exclus par les .gitignore, les caches et dossiers d'outils cachés (.venv, .gradle…), les produits de compilation (DerivedData, Pods, .app, .framework…) et les environnements Python.
- Alertes CVE : filtres par projet, par gravité et « Non lues seulement », et pagination par 20 alertes.
- Style du héraut : l'ange annonce les nouvelles de vos projets dans la vue d'ensemble et dans l'onglet Général.
- Thème sombre : dans les Réglages, section Apparence, choisissez Clair, Sombre ou Système (suit l'apparence de macOS, y compris quand elle change). Le choix est gardé d'une ouverture à l'autre.
- Panneau de la note épuré : la lettre en très grand, la note sur 100 et une phrase (évolution, palier suivant et correction qui suffit à l'atteindre, ou effet des corrections cochées), puis la relance et les exports PDF et Markdown.
- Dépendances : un clic sur un paquet (ou sur « Corriger ») ouvre sa fiche, avec ses vulnérabilités et la commande adaptée à l'outil du projet (npm, yarn, pnpm, bun, cargo, pip, poetry, uv, pipenv, go, composer). « Corriger » l'exécute dans le dossier du projet en affichant la sortie, puis propose de relancer l'analyse.
- Vue d'ensemble en tableau : les projets rangés en trois colonnes (urgent, à améliorer, en bonne santé), chacun avec sa note, son évolution, la courbe de ses notes, ses critiques, problèmes et alertes CVE ; le héraut annonce les projets en urgence et ceux en bonne santé.
- Pendant l'analyse, la progression s'affiche dans le bandeau du héraut de l'onglet Général (« Le héraut analyse ») : étape réelle du moteur, fichiers lus sur le total, temps écoulé, problèmes trouvés, dernier fichier lu et barre de progression ; sur les autres pages, une carte discrète en haut donne les mêmes informations. La page reste utilisable dessous.

## 0.1.9
- Pendant une analyse, une barre en bas de la fenêtre montre la mémoire et le processeur utilisés par Herald, avec leur courbe et le pic.
- Connexion avec Google ou GitHub fiable sous Firefox.
- Herald est gratuit : 100 analyses par jour pour tous et toutes les catégories détaillées, sans abonnement.
- Le code de l'app est open source : github.com/async-herald/herald.

## 0.1.8
- Barre des ressources pendant l'analyse, correctif de connexion sous Firefox, fin de l'abonnement.

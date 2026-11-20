//! Textes français des règles : un titre clair, pourquoi c'est un problème et
//! comment le corriger. Le message du moteur reste affiché à côté : il porte
//! le détail de chaque relevé (valeur mesurée, paquet, version…).
//!
//! Une règle absente d'ici (règle personnelle, nouvelle règle du catalogue)
//! garde un titre tiré de son identifiant et le message du moteur.

/// Textes d'une règle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleText {
    pub title: &'static str,
    /// Pourquoi c'est un problème, en une ou deux phrases.
    pub why: &'static str,
    /// Comment le corriger.
    pub fix: &'static str,
}

const fn t(title: &'static str, why: &'static str, fix: &'static str) -> RuleText {
    RuleText { title, why, fix }
}

/// Titre français d'une règle, ou titre tiré de son identifiant.
pub fn rule_title(id: &str) -> String {
    rule_text(id).map_or_else(|| super::humanize_rule(id), |text| text.title.to_owned())
}

/// Textes français d'une règle connue.
pub fn rule_text(id: &str) -> Option<RuleText> {
    Some(match id {
        // ── Sécurité ──────────────────────────────────────────────────────
        "sql-string-format" => t(
            "Requête SQL construite par formatage",
            "La requête SQL est fabriquée en collant une valeur dans le texte (format!, f-string, Sprintf). Si cette valeur vient d'un utilisateur, il peut y glisser du SQL et lire, modifier ou effacer la base.",
            "Gardez la requête fixe et passez la valeur en paramètre lié : $1 ou ? dans le SQL, puis .bind(valeur).",
        ),
        "sql_injection" => t(
            "Injection SQL possible",
            "La requête est assemblée par concaténation de texte. Une valeur malveillante peut changer le sens de la requête et donner accès à toute la base.",
            "Utilisez une requête paramétrée : la valeur est envoyée à part et n'est jamais interprétée comme du SQL.",
        ),
        "taint-sql-injection" => t(
            "Entrée utilisateur dans une requête SQL",
            "Une valeur reçue de l'extérieur (requête HTTP, formulaire) arrive telle quelle dans une requête SQL exécutée. C'est une injection SQL exploitable.",
            "Passez la valeur en paramètre de la requête (placeholder) au lieu de la concaténer.",
        ),
        "nosql_injection" => t(
            "Injection NoSQL possible",
            "Une entrée non vérifiée arrive dans un filtre de base NoSQL (MongoDB…). Un objet comme {\"$ne\": null} peut contourner une vérification de mot de passe.",
            "Vérifiez le type de l'entrée (chaîne, nombre) avant de l'utiliser et n'utilisez jamais $where avec une donnée reçue.",
        ),
        "command_injection" => t(
            "Injection de commande possible",
            "Une commande système est construite à partir d'une valeur variable. Une valeur comme « ; rm -rf / » ferait exécuter une seconde commande.",
            "Passez les arguments sous forme de liste, sans shell, et n'acceptez que des valeurs attendues.",
        ),
        "taint-command-injection" => t(
            "Entrée utilisateur dans une commande système",
            "Une valeur reçue de l'extérieur arrive dans exec, spawn ou os.system. Un attaquant peut faire exécuter n'importe quelle commande sur le serveur.",
            "Évitez le shell : appelez le programme avec une liste d'arguments, et validez l'entrée contre une liste de valeurs permises.",
        ),
        "taint-code-injection" => t(
            "Entrée utilisateur exécutée comme du code",
            "Une valeur reçue de l'extérieur arrive dans eval, Function ou exec. N'importe qui peut alors faire exécuter son propre code par l'application.",
            "N'évaluez jamais une entrée. Pour lire des données, utilisez un parseur (JSON.parse, par exemple).",
        ),
        "taint-xss" => t(
            "Entrée utilisateur injectée dans la page (XSS)",
            "Une valeur reçue de l'extérieur est insérée dans le HTML de la page. Un script caché dans cette valeur s'exécuterait chez vos visiteurs et pourrait voler leur session.",
            "Insérez le texte avec textContent, ou nettoyez le HTML avec une bibliothèque comme DOMPurify.",
        ),
        "no_inner_html" => t(
            "Affectation directe de innerHTML",
            "Écrire du HTML avec innerHTML ou outerHTML exécute les scripts qu'il contient. Avec une donnée externe, c'est une faille XSS.",
            "Utilisez textContent pour du texte, ou créez les éléments avec createElement.",
        ),
        "document-write" => t(
            "Utilisation de document.write()",
            "document.write réécrit la page et injecte du HTML brut : une donnée non maîtrisée devient une faille XSS. La fonction bloque aussi le chargement.",
            "Construisez le contenu avec createElement et textContent.",
        ),
        "no_eval" => t(
            "Utilisation de eval() ou exec()",
            "eval et exec exécutent du texte comme du code. Si ce texte peut venir de l'extérieur, l'application exécute le code d'un attaquant.",
            "Remplacez eval par une solution sans exécution : JSON.parse pour des données, une table de correspondance pour choisir une action.",
        ),
        "settimeout-string" => t(
            "setTimeout ou setInterval avec du texte",
            "Passer une chaîne à setTimeout revient à appeler eval : le texte est exécuté comme du code.",
            "Passez une fonction : setTimeout(() => maFonction(), 1000).",
        ),
        "hardcoded_secret" => t(
            "Secret écrit dans le code",
            "Une clé d'API, un jeton ou un identifiant est écrit en clair. Toute personne qui voit le code, ou l'historique Git, peut l'utiliser.",
            "Révoquez ce secret, puis lisez-le depuis une variable d'environnement ou un gestionnaire de secrets.",
        ),
        "hardcoded_password" => t(
            "Mot de passe écrit dans le code",
            "Le mot de passe est visible par toute personne qui lit le code ou l'historique Git, et il est le même partout où le code tourne.",
            "Changez ce mot de passe, puis lisez-le depuis une variable d'environnement ou un gestionnaire de secrets.",
        ),
        "leaked-secret" => t(
            "Jeton d'accès en clair",
            "Un jeton reconnu (GitHub, AWS, Slack, Stripe, ou des identifiants dans une URL) est dans le code. Des robots parcourent les dépôts publics pour les récupérer en quelques minutes.",
            "Révoquez le jeton tout de suite chez le fournisseur, puis passez par une variable d'environnement.",
        ),
        "hardcoded-private-key" => t(
            "Clé privée écrite dans le code",
            "Une clé privée (BEGIN PRIVATE KEY) permet de se faire passer pour votre serveur ou de déchiffrer vos échanges. Elle ne doit jamais être dans le dépôt.",
            "Remplacez la clé (elle est compromise) et chargez la nouvelle depuis un gestionnaire de secrets.",
        ),
        "private-key-block" => t(
            "Fichier de clé privée versionné",
            "Une clé privée est enregistrée dans un fichier du dépôt. Elle reste dans l'historique Git même après suppression du fichier.",
            "Retirez le fichier, ajoutez-le au .gitignore et remplacez la clé, qui doit être considérée comme compromise.",
        ),
        "committed-env-file" => t(
            "Fichier .env versionné",
            "Le fichier .env contient en général les mots de passe et clés de l'application. Le versionner les expose à toute personne qui a accès au dépôt.",
            "Retirez-le du dépôt (git rm --cached .env), ajoutez-le au .gitignore et changez les secrets qu'il contenait.",
        ),
        "committed-secret-file" => t(
            "Fichier de secrets versionné",
            "Un fichier de clés ou d'identifiants (.pem, .p12, credentials…) est dans le dépôt.",
            "Retirez-le du dépôt, ajoutez-le au .gitignore et révoquez ce qu'il contient.",
        ),
        "hardcoded-internal-url" => t(
            "Adresse interne écrite dans le code",
            "Une URL d'environnement précis (staging, réseau interne, VPN) est figée dans le code : elle révèle votre infrastructure et casse dès que l'adresse change.",
            "Mettez cette adresse dans la configuration ou une variable d'environnement.",
        ),
        "weak_crypto" => t(
            "Chiffrement ou hachage obsolète",
            "MD5, SHA-1, DES et RC4 sont cassés : on peut produire des collisions ou retrouver les données en peu de temps.",
            "Utilisez SHA-256 ou plus pour un hachage, AES-GCM pour chiffrer, et bcrypt ou Argon2 pour les mots de passe.",
        ),
        "insecure-random" => t(
            "Aléatoire prévisible pour une valeur sensible",
            "Math.random et les générateurs classiques sont prévisibles. Un jeton, un code ou un sel créé ainsi peut être deviné.",
            "Utilisez un générateur cryptographique : crypto.randomBytes, secrets en Python, rand::rngs::OsRng en Rust.",
        ),
        "insecure-jwt" => t(
            "Jeton JWT accepté sans vérification",
            "La signature du jeton n'est pas vérifiée, ou l'algorithme « none » est autorisé. N'importe qui peut fabriquer un jeton et se connecter en tant que n'importe quel utilisateur.",
            "Vérifiez toujours la signature avec un algorithme fixe et explicite (HS256 ou RS256).",
        ),
        "jwt-no-expiry" => t(
            "Jeton JWT sans date d'expiration",
            "Un jeton sans expiration reste valable pour toujours : s'il est volé, il donne un accès permanent.",
            "Donnez une durée de vie courte au jeton (expiresIn) et renouvelez-le avec un jeton de rafraîchissement.",
        ),
        "insecure_cookie" => t(
            "Cookie sans protections",
            "Un cookie sans HttpOnly, Secure ou SameSite peut être lu par un script, envoyé en clair ou utilisé depuis un autre site.",
            "Ajoutez HttpOnly, Secure et SameSite=Lax (ou Strict) aux cookies de session.",
        ),
        "insecure_transport" => t(
            "Adresse en HTTP non chiffré",
            "Les échanges en http:// circulent en clair : toute personne sur le réseau peut les lire ou les modifier.",
            "Utilisez https://.",
        ),
        "tls_verification_disabled" => t(
            "Vérification du certificat TLS désactivée",
            "Sans vérification du certificat, la connexion accepte n'importe quel serveur, y compris un intermédiaire malveillant qui lit et modifie le trafic.",
            "Réactivez la vérification. Pour un certificat interne, ajoutez son autorité de certification au lieu de tout désactiver.",
        ),
        "permissive_cors" => t(
            "CORS ouvert à tous les sites",
            "Access-Control-Allow-Origin vaut « * » ou recopie l'origine reçue : n'importe quel site peut appeler votre API depuis le navigateur de vos visiteurs.",
            "Listez explicitement les origines autorisées.",
        ),
        "cors-credentialed-wildcard" => t(
            "CORS ouvert à tous avec les cookies",
            "Toutes les origines sont acceptées alors que les cookies sont envoyés : un autre site peut agir au nom de vos utilisateurs connectés.",
            "N'autorisez qu'une liste fixe d'origines de confiance quand credentials est activé.",
        ),
        "open-redirect" => t(
            "Redirection vers une adresse choisie par l'utilisateur",
            "La destination de la redirection vient de la requête. Un lien piégé vers votre site peut renvoyer vers un faux site, ce qui sert à l'hameçonnage.",
            "N'acceptez que des chemins relatifs ou des destinations d'une liste autorisée.",
        ),
        "path-traversal" => t(
            "Chemin de fichier construit avec une entrée",
            "Un chemin de fichier dépend d'une valeur reçue. Avec « ../ », un attaquant peut lire ou écrire en dehors du dossier prévu.",
            "Normalisez le chemin (canonicalize) et vérifiez qu'il reste dans le dossier de base.",
        ),
        "zip-slip" => t(
            "Extraction d'archive sans contrôle des chemins",
            "Une archive peut contenir des fichiers nommés « ../../… ». Les extraire sans contrôle écrit des fichiers n'importe où sur le disque.",
            "Pour chaque entrée, calculez le chemin final et refusez-la s'il sort du dossier cible.",
        ),
        "ssrf" => t(
            "Requête vers une adresse choisie par l'utilisateur (SSRF)",
            "Le serveur appelle une URL construite à partir d'une entrée. Un attaquant peut lui faire interroger des services internes, inaccessibles depuis l'extérieur.",
            "Vérifiez l'hôte contre une liste autorisée et bloquez les adresses internes.",
        ),
        "ssti" => t(
            "Gabarit construit avec une entrée (SSTI)",
            "Un template est fabriqué à partir d'une donnée reçue. Le moteur de templates peut alors exécuter du code sur le serveur.",
            "Gardez les templates fixes et passez les données comme variables.",
        ),
        "xxe" => t(
            "Lecteur XML vulnérable (XXE)",
            "Le parseur XML accepte les entités externes. Un document piégé peut lire des fichiers du serveur ou appeler des services internes.",
            "Désactivez les DTD et les entités externes dans la configuration du parseur.",
        ),
        "unsafe_deserialization" => t(
            "Désérialisation non sûre",
            "Charger des données avec pickle, yaml.load ou équivalent peut exécuter du code caché dans ces données.",
            "Utilisez un format sans exécution (JSON) ou un chargeur sûr comme yaml.safe_load.",
        ),
        "prototype-pollution" => t(
            "Pollution de prototype possible",
            "Une donnée externe est fusionnée récursivement ou utilise des clés comme « __proto__ ». Elle peut modifier le comportement de tous les objets de l'application.",
            "Refusez les clés __proto__, constructor et prototype, ou créez les objets avec Object.create(null).",
        ),
        "mass-assignment" => t(
            "Tous les champs reçus sont enregistrés",
            "Le contenu entier de la requête est copié dans un modèle. Un utilisateur peut ajouter un champ comme isAdmin et se donner des droits.",
            "Listez explicitement les champs acceptés.",
        ),
        "missing-auth-guard" => t(
            "Route sans vérification d'authentification",
            "Cette route modifie des données mais aucune vérification de connexion n'y apparaît. N'importe qui pourrait l'appeler.",
            "Ajoutez la vérification d'authentification (middleware, extracteur, Depends) ou confirmez que la route est volontairement publique.",
        ),
        "insecure-file-permissions" => t(
            "Fichier modifiable par tout le monde",
            "Les droits accordent l'écriture à tous les utilisateurs de la machine, qui peuvent modifier le fichier.",
            "Restreignez les droits, par exemple 0o600 pour un fichier privé ou 0o644 pour un fichier lisible.",
        ),
        "insecure-temp-api" => t(
            "Fichier temporaire au nom prévisible",
            "L'API utilisée crée un nom de fichier temporaire devinable. Un autre programme peut créer le fichier avant vous et détourner l'écriture.",
            "Utilisez mkstemp, NamedTemporaryFile ou os.CreateTemp, qui créent un fichier unique.",
        ),
        "insecure-temp-path" => t(
            "Chemin temporaire écrit en dur",
            "Un chemin fixe dans /tmp peut être créé d'avance par un autre programme, qui pourrait alors lire ou remplacer vos données.",
            "Créez un fichier temporaire unique avec l'API du langage (mkstemp, tempfile).",
        ),
        "dynamic-require" => t(
            "Module chargé depuis un chemin variable",
            "Le chemin du module n'est pas écrit en dur : on ne peut pas savoir ce qui sera chargé. S'il dépend d'une entrée, un attaquant peut faire charger son propre code.",
            "Chargez des modules à chemin fixe, ou choisissez parmi une liste connue.",
        ),
        "regex-injection" => t(
            "Expression régulière construite avec une entrée",
            "Une regex fabriquée à partir d'une donnée externe peut bloquer le serveur (ReDoS) ou trouver autre chose que prévu.",
            "Échappez l'entrée avant de l'utiliser dans la regex, ou validez-la contre un modèle fixe.",
        ),
        "php-superglobal-input" => t(
            "Lecture directe de $_GET ou $_POST",
            "La valeur est lue sans filtrage dans une superglobale : rien ne garantit son type ni son contenu, et le code est difficile à tester.",
            "Passez par l'objet Request du framework ou par filter_input() avec un filtre adapté.",
        ),
        "vulnerable-dependency" => t(
            "Dépendance avec une faille connue",
            "La version utilisée de ce paquet a une vulnérabilité publiée (CVE ou avis GitHub). Les failles publiées sont les premières exploitées.",
            "Passez à la version corrigée indiquée, puis relancez vos tests.",
        ),

        // ── Qualité ───────────────────────────────────────────────────────
        "long-line" => t(
            "Ligne trop longue",
            "Une ligne très longue oblige à défiler horizontalement et se lit mal, surtout dans une revue de code.",
            "Coupez la ligne. Un formateur automatique (rustfmt, Prettier, Black) le fait pour vous.",
        ),
        "function-too-long" => t(
            "Fonction trop longue",
            "Une fonction longue fait plusieurs choses à la fois : elle est difficile à comprendre, à tester et à modifier sans casser autre chose.",
            "Découpez-la en petites fonctions qui ont chacune un nom clair.",
        ),
        "max_file_lines" => t(
            "Fichier trop long",
            "Un fichier très long mélange souvent plusieurs sujets et devient difficile à parcourir.",
            "Répartissez son contenu dans plusieurs modules, un par sujet.",
        ),
        "too-many-params" => t(
            "Trop de paramètres",
            "Avec beaucoup de paramètres, on confond facilement leur ordre et chaque appel devient difficile à lire.",
            "Regroupez les paramètres liés dans un objet ou une struct.",
        ),
        "deep-nesting" => t(
            "Code trop imbriqué",
            "Plusieurs niveaux de if et de boucles imbriqués rendent le chemin d'exécution difficile à suivre.",
            "Sortez tôt avec des clauses de garde (if … return) et extrayez les blocs internes en fonctions.",
        ),
        "high-complexity" => t(
            "Fonction trop complexe",
            "La fonction a beaucoup de chemins possibles (conditions, boucles) : il faut beaucoup de tests pour tous les couvrir et les bugs s'y cachent facilement.",
            "Extrayez des sous-fonctions et simplifiez les conditions.",
        ),
        "high-cognitive-complexity" => t(
            "Fonction difficile à suivre",
            "L'enchaînement des conditions et des imbrications demande trop d'effort pour être compris d'une seule lecture.",
            "Aplatissez avec des retours anticipés et extrayez les parties imbriquées.",
        ),
        "commented-out-code" => t(
            "Code mis en commentaire",
            "Du code désactivé encombre le fichier et on ne sait plus s'il faut le garder. Git conserve déjà les anciennes versions.",
            "Supprimez-le.",
        ),
        "todo_fixme" => t(
            "TODO ou FIXME oublié",
            "Ce marqueur signale un travail non terminé, qui a tendance à rester là indéfiniment.",
            "Faites le travail, ou créez un ticket et retirez le commentaire.",
        ),
        "empty-catch" => t(
            "Bloc catch vide",
            "L'erreur est attrapée puis ignorée : le programme continue comme si de rien n'était et le problème devient invisible.",
            "Journalisez l'erreur, traitez-la, ou laissez-la remonter.",
        ),
        "swallowed-error" => t(
            "Erreur ignorée",
            "Une erreur est attrapée sans être traitée ni journalisée (except: pass, if err != nil {} vide, unwrap_or_default). Quand ça casse, rien ne permet de comprendre pourquoi.",
            "Journalisez l'erreur ou faites-la remonter à l'appelant.",
        ),
        "useless-catch-rethrow" => t(
            "catch qui ne fait que relancer",
            "Attraper une erreur pour la relancer telle quelle n'apporte rien et alourdit le code.",
            "Supprimez le try/catch, ou ajoutez un vrai traitement (journal, contexte).",
        ),
        "return-in-finally" => t(
            "return dans un bloc finally",
            "Un return, throw ou break dans finally remplace le résultat ou l'erreur du try : une exception disparaît sans bruit.",
            "Sortez ce return du bloc finally.",
        ),
        "no-unwrap-in-prod" => t(
            "unwrap() ou expect() dans du code de production",
            "Si la valeur est absente ou en erreur, le programme s'arrête net (panic) : une requête renvoie une erreur 500 ou un service tombe.",
            "Propagez l'erreur avec ?, traitez-la avec match, ou fournissez une valeur par défaut.",
        ),
        "unhandled-promise" => t(
            "Promesse sans gestion d'erreur",
            "Un .then() sans .catch() : si la promesse échoue, l'erreur n'est capturée nulle part et peut arrêter le processus Node.",
            "Ajoutez un .catch(), ou utilisez await dans un try/catch.",
        ),
        "unreachable-code" => t(
            "Code jamais exécuté",
            "Ce code suit un return, un throw ou un break : il ne s'exécute jamais et trompe la personne qui lit.",
            "Supprimez-le.",
        ),
        "no_unused_vars" => t(
            "Variable inutilisée",
            "La variable est déclarée mais jamais lue : c'est un reste de code ou un oubli.",
            "Supprimez-la, ou préfixez son nom par _ si elle doit rester.",
        ),
        "unused-import" => t(
            "Import inutilisé",
            "L'import n'est utilisé nulle part dans le fichier. Il encombre et peut alourdir le programme.",
            "Supprimez-le.",
        ),
        "unused-parameter" => t(
            "Paramètre inutilisé",
            "Le paramètre est reçu mais jamais utilisé : les appelants passent une valeur pour rien.",
            "Supprimez-le, ou préfixez-le par _ s'il est imposé par une interface.",
        ),
        "unused-export" => t(
            "Export jamais utilisé",
            "Cet élément est exporté mais aucun autre fichier ne l'importe : c'est probablement du code mort.",
            "Supprimez l'export, ou l'élément entier s'il ne sert plus.",
        ),
        "orphan-module" => t(
            "Fichier jamais importé",
            "Aucun autre fichier n'importe ce module : il n'est probablement plus utilisé.",
            "Vérifiez qu'il ne sert plus, puis supprimez-le.",
        ),
        "no_debug_print" => t(
            "Affichage de débogage oublié",
            "Un print, console.log ou dbg! oublié pollue les journaux et peut afficher des données sensibles.",
            "Supprimez-le, ou remplacez-le par un vrai journal (logger) avec un niveau adapté.",
        ),
        "no_alert" => t(
            "alert(), confirm() ou prompt()",
            "Ces boîtes de dialogue bloquent la page et gênent l'utilisateur. Ce sont souvent des restes de débogage.",
            "Utilisez une fenêtre ou un message intégré à l'interface.",
        ),
        "no_var" => t(
            "Déclaration avec var",
            "var a une portée surprenante (toute la fonction) qui provoque des bugs difficiles à trouver.",
            "Utilisez const, ou let si la valeur change.",
        ),
        "loose-equality" => t(
            "Comparaison avec == au lieu de ===",
            "== convertit les types avant de comparer : 0 == '' est vrai, ce qui cause des bugs discrets.",
            "Utilisez === et !==.",
        ),
        "compare-nan" => t(
            "Comparaison directe avec NaN",
            "NaN n'est égal à rien, pas même à lui-même : x === NaN est toujours faux.",
            "Utilisez Number.isNaN(x).",
        ),
        "float-equality" => t(
            "Égalité exacte entre nombres décimaux",
            "Les nombres à virgule sont arrondis en binaire : 0,1 + 0,2 ne vaut pas exactement 0,3. Une comparaison exacte échoue au hasard.",
            "Comparez avec une tolérance : abs(a - b) < epsilon.",
        ),
        "assignment-in-condition" => t(
            "Affectation dans une condition",
            "if (x = y) affecte au lieu de comparer : c'est presque toujours un == oublié, et la condition devient toujours vraie ou toujours fausse.",
            "Corrigez la comparaison, ou sortez l'affectation de la condition.",
        ),
        "unsafe-negation" => t(
            "Négation mal placée",
            "!a instanceof B se lit (!a) instanceof B : le test est toujours faux.",
            "Ajoutez des parenthèses : !(a instanceof B).",
        ),
        "redundant-boolean" => t(
            "Booléen inutile",
            "Comparer à true ou renvoyer true/false selon une condition alourdit le code sans rien apporter.",
            "Utilisez la condition directement : if (x), return condition.",
        ),
        "nested-ternary" => t(
            "Conditions ternaires imbriquées",
            "Plusieurs ? : enchaînés sont très difficiles à lire.",
            "Remplacez-les par un if/else ou une petite fonction.",
        ),
        "else-after-return" => t(
            "else inutile après un return",
            "Le if se termine déjà par un return : le else ajoute un niveau d'indentation pour rien.",
            "Supprimez le else et désindentez son contenu.",
        ),
        "magic-number" => t(
            "Nombre sans explication",
            "Un nombre écrit tel quel dans une comparaison ne dit pas ce qu'il représente (un délai, une limite, un code).",
            "Mettez-le dans une constante au nom explicite.",
        ),
        "magic-sleep" => t(
            "Attente fixe codée en dur",
            "Attendre une durée fixe (sleep) en espérant qu'une ressource soit prête est fragile : trop court, ça échoue ; trop long, ça ralentit.",
            "Attendez un vrai signal : événement, réponse, ou nouvel essai avec un délai croissant.",
        ),
        "naming-conventions" => t(
            "Nom hors convention",
            "Ce nom ne suit pas l'usage du langage (camelCase, PascalCase, snake_case) : le code paraît incohérent.",
            "Renommez selon la convention du langage.",
        ),
        "blocking-sync-io" => t(
            "Lecture ou écriture bloquante",
            "Un appel de fichier synchrone bloque tout le serveur Node pendant sa durée : les autres requêtes attendent.",
            "Utilisez la version asynchrone (fs.promises avec await).",
        ),
        "blocking-sync-crypto" => t(
            "Calcul cryptographique bloquant",
            "bcrypt, pbkdf2 ou scrypt en version synchrone bloquent le serveur pendant le calcul, qui est volontairement lent.",
            "Utilisez la version asynchrone.",
        ),
        "io-in-loop" => t(
            "Appel réseau ou base de données dans une boucle",
            "Chaque tour de boucle attend sa réponse : 100 éléments font 100 allers-retours (problème N+1).",
            "Regroupez les appels en une seule requête, ou lancez-les en parallèle (Promise.all).",
        ),
        "quadratic-loop" => t(
            "Recherche répétée dans une boucle",
            "Chercher dans une liste à chaque tour de boucle fait un travail qui grandit au carré : lent dès qu'il y a beaucoup de données.",
            "Construisez une Map ou un Set avant la boucle pour chercher instantanément.",
        ),
        "spread-in-reducer" => t(
            "Copie de l'accumulateur dans reduce",
            "{...acc} recopie tout l'accumulateur à chaque tour : le traitement devient très lent sur une grande liste.",
            "Modifiez l'accumulateur directement (acc[k] = v, push), ou utilisez une boucle.",
        ),
        "json-deep-clone" => t(
            "Copie d'objet via JSON",
            "JSON.parse(JSON.stringify(x)) est lent et perd les dates, Map, Set et undefined.",
            "Utilisez structuredClone(x).",
        ),
        "heavy-barrel-import" => t(
            "Import complet d'une grosse bibliothèque",
            "Importer tout lodash ou moment alourdit fortement le code envoyé au navigateur.",
            "Importez seulement la fonction utile (lodash/get) ou une alternative légère (dayjs).",
        ),
        "empty-check-idiom" => t(
            "Test de vide peu lisible",
            "Comparer une taille à 0 est moins clair qu'une méthode dédiée, et parfois plus lent.",
            "Utilisez .isEmpty() ou l'équivalent du langage.",
        ),
        "linq-count-vs-any" => t(
            ".Count() > 0 au lieu de .Any()",
            ".Count() parcourt toute la séquence alors que .Any() s'arrête au premier élément.",
            "Utilisez .Any().",
        ),
        "http-no-timeout" => t(
            "Appel HTTP sans délai maximal",
            "Si le serveur distant ne répond pas, la tâche attend indéfiniment et peut bloquer l'application.",
            "Créez un client avec un timeout explicite.",
        ),
        "sql-select-star" => t(
            "SELECT * dans une requête",
            "Récupérer toutes les colonnes charge des données inutiles et casse le code si une colonne est ajoutée.",
            "Listez les colonnes nécessaires.",
        ),
        "css-important-overuse" => t(
            "!important dans le CSS",
            "!important force un style et oblige le suivant à en rajouter un : la feuille de style devient impossible à maîtriser.",
            "Augmentez plutôt la précision du sélecteur, ou réorganisez les règles.",
        ),
        "img-missing-alt" => t(
            "Image sans texte alternatif",
            "Sans attribut alt, les lecteurs d'écran ne peuvent pas décrire l'image aux personnes malvoyantes.",
            "Ajoutez alt=\"description\", ou alt=\"\" pour une image décorative.",
        ),
        "missing-html-lang" => t(
            "Langue de la page non précisée",
            "Sans lang sur <html>, les lecteurs d'écran prononcent mal le texte et les moteurs de recherche devinent la langue.",
            "Ajoutez lang=\"fr\" (ou la langue de la page) sur la balise <html>.",
        ),
        "jsx-missing-key" => t(
            "Liste React sans clé",
            "Sans prop key stable, React mélange les éléments quand la liste change : saisies perdues, état au mauvais endroit.",
            "Ajoutez key={element.id} sur chaque élément de la liste.",
        ),
        "wildcard_import" => t(
            "Import de tout un module (import *)",
            "from module import * ajoute des noms invisibles : on ne sait plus d'où vient une fonction, et des noms peuvent s'écraser.",
            "Importez explicitement les noms utilisés.",
        ),
        "php-count-in-loop-condition" => t(
            "count() recalculé à chaque tour de boucle",
            "La taille est recalculée à chaque itération alors qu'elle ne change pas.",
            "Calculez-la une fois avant la boucle, ou utilisez foreach.",
        ),
        "php-deprecated-function" => t(
            "Fonction PHP obsolète",
            "Cette fonction est dépréciée ou supprimée : le code cassera à la prochaine mise à jour de PHP.",
            "Remplacez-la par son équivalent moderne (PDO, preg_*, IntlDateFormatter…).",
        ),
        "php-error-suppression" => t(
            "Erreur masquée avec @",
            "L'opérateur @ cache l'erreur sans la traiter : le code continue avec une valeur invalide et rien n'est journalisé.",
            "Retirez le @ et testez la valeur de retour, ou attrapez l'exception.",
        ),

        // ── Conception (Pattern) ─────────────────────────────────────────
        "dry" => t(
            "Code dupliqué",
            "Le même bloc existe à plusieurs endroits : une correction faite à un endroit est souvent oubliée aux autres.",
            "Regroupez la logique dans une fonction commune.",
        ),
        "solid" => t(
            "Principe de conception SOLID non respecté",
            "La classe ou le module mélange des responsabilités, ou dépend de détails qu'il devrait ignorer : il devient difficile à faire évoluer.",
            "Séparez les responsabilités et faites dépendre le code d'interfaces plutôt que d'implémentations.",
        ),
        "mvc" => t(
            "Couches mélangées",
            "Accès aux données ou affichage se trouvent dans la logique applicative : chaque changement touche plusieurs couches à la fois.",
            "Isolez l'accès aux données (modèle) et l'affichage (vue) hors du contrôleur.",
        ),
        "god-class" => t(
            "Classe qui fait tout",
            "Une classe avec énormément de méthodes concentre trop de responsabilités : chaque modification risque d'en casser une autre.",
            "Répartissez ses responsabilités dans des classes plus petites.",
        ),
        "high-coupling" => t(
            "Module trop dépendant",
            "Ce module importe beaucoup d'autres modules : il casse dès que l'un d'eux change.",
            "Réduisez ses dépendances, par exemple en le découpant.",
        ),
        "import-cycle" => t(
            "Imports circulaires",
            "Des fichiers s'importent mutuellement en boucle : ordre de chargement fragile, valeurs parfois undefined, code difficile à séparer.",
            "Déplacez la partie commune dans un module à part que les deux importent.",
        ),
        "deep-relative-import" => t(
            "Import relatif très profond",
            "Un import en ../../../ traverse plusieurs dossiers : il casse dès qu'un fichier est déplacé.",
            "Utilisez un alias de chemin ou rapprochez les modules.",
        ),
        "deep-import-internals" => t(
            "Import des fichiers internes d'un module",
            "Importer /internal/, /src/ ou /dist/ d'un autre module contourne son interface publique et casse à sa prochaine réorganisation.",
            "Importez depuis le point d'entrée public du module.",
        ),
        "barrel-wildcard-reexport" => t(
            "Ré-export en bloc (export *)",
            "export * cache ce qui est réellement public et alourdit le chargement.",
            "Ré-exportez nommément ce qui doit être public.",
        ),
        "feature-envy" => t(
            "Accès en cascade à d'autres objets",
            "Une chaîne comme a.b.c.d.e montre que ce code dépend de la structure interne d'autres objets.",
            "Déplacez cette logique dans l'objet qui possède les données, ou ajoutez-lui une méthode dédiée.",
        ),
        "global-mutable-state" => t(
            "État global modifiable",
            "Une variable exportée et modifiable est partagée par tout le programme : son contenu dépend de l'ordre d'exécution et provoque des bugs difficiles à reproduire.",
            "Exposez des fonctions pour lire et modifier cet état, ou passez-le en paramètre.",
        ),
        "symfony-service-locator" => t(
            "Service récupéré depuis le conteneur",
            "Appeler le conteneur directement cache les dépendances de la classe et la rend difficile à tester.",
            "Injectez le service par le constructeur.",
        ),

        // ── Tests ─────────────────────────────────────────────────────────
        "no-focused-tests" => t(
            "Test isolé (.only) laissé en place",
            "Avec .only, fdescribe ou fit, seuls ces tests tournent : tous les autres sont ignorés sans bruit, y compris en intégration continue.",
            "Retirez .only (ou le f) avant de commiter.",
        ),
        "no-skipped-tests" => t(
            "Test désactivé",
            "Un test ignoré (.skip, xit, skip) donne une fausse impression de couverture.",
            "Réparez le test ou supprimez-le.",
        ),
        "no-empty-test" => t(
            "Test vide",
            "Un test sans contenu passe toujours au vert sans rien vérifier.",
            "Ajoutez des vérifications, ou supprimez le test.",
        ),
        "placeholder-assertion" => t(
            "Vérification qui ne teste rien",
            "expect(true).toBe(true) passe toujours : le test ne vérifie pas le code.",
            "Vérifiez le vrai résultat de la fonction testée.",
        ),

        // ── Dépendances ───────────────────────────────────────────────────
        "outdated-dependency" => t(
            "Dépendance pas à jour",
            "Une version plus récente du paquet existe. Les vieilles versions accumulent les bugs connus et rendent les futures mises à jour plus difficiles.",
            "Mettez à jour vers la version indiquée et relancez vos tests.",
        ),
        "unpinned-dependency-wildcard" => t(
            "Version de dépendance non fixée",
            "Avec « * » ou « latest », une nouvelle version peut s'installer sans que personne ne l'ait choisie, et casser la compilation.",
            "Indiquez une version précise (ou une plage limitée comme ^1.4.0).",
        ),
        "duplicate-dependency-version" => t(
            "Même dépendance en plusieurs versions",
            "Le paquet est demandé avec des versions différentes selon les dossiers : le comportement change selon l'endroit.",
            "Alignez la version dans tous les fichiers de dépendances.",
        ),
        "multiple-lockfiles" => t(
            "Plusieurs fichiers de verrouillage",
            "package-lock.json, yarn.lock et pnpm-lock.yaml coexistent : chaque outil installe des versions différentes.",
            "Choisissez un seul gestionnaire de paquets et supprimez les autres fichiers de verrouillage.",
        ),

        // ── Versioning (Git) ─────────────────────────────────────────────
        "merge-conflict" => t(
            "Conflit de fusion non résolu",
            "Des marqueurs <<<<<<< ou >>>>>>> sont restés dans le fichier : le code ne compile pas ou se comporte mal.",
            "Résolvez le conflit et supprimez les marqueurs.",
        ),
        "conventional-commit" => t(
            "Message de commit hors convention",
            "Sans préfixe (feat:, fix:, docs:…), l'historique est difficile à parcourir et on ne peut pas générer de journal des modifications.",
            "Commencez vos messages par feat:, fix:, chore:, docs:, refactor: ou test:.",
        ),
        "commit-too-long" => t(
            "Titre de commit trop long",
            "Au-delà de 72 caractères, le titre est coupé dans la plupart des outils.",
            "Résumez en une ligne courte et mettez le détail dans le corps du message.",
        ),
        "no-wip-commit" => t(
            "Commit « en cours » (wip)",
            "Les commits wip, fixup ou tmp encombrent l'historique partagé.",
            "Fusionnez-les (squash) avant de pousser.",
        ),
        "branch-naming" => t(
            "Nom de branche hors convention",
            "Un nom de branche sans préfixe ne dit pas ce qu'elle contient.",
            "Utilisez un préfixe : feature/, fix/, hotfix/ ou release/.",
        ),
        "no-commit-on-main" => t(
            "Travail directement sur main",
            "Travailler sur main sans branche dédiée empêche la relecture avant fusion.",
            "Créez une branche et ouvrez une pull request.",
        ),
        "build-output-committed" => t(
            "Dossier de compilation versionné",
            "Les fichiers générés (dist, build, target) gonflent le dépôt et créent des conflits inutiles.",
            "Ajoutez ce dossier au .gitignore et retirez-le du dépôt.",
        ),
        "junk-committed" => t(
            "Fichier parasite versionné",
            "Des fichiers système ou temporaires (.DS_Store, .bak, .pyc, .log) n'ont rien à faire dans le dépôt.",
            "Supprimez-les et ajoutez leur extension au .gitignore.",
        ),
        "large-artifact-committed" => t(
            "Gros fichier binaire versionné",
            "Une archive, un dump ou un binaire volumineux alourdit chaque clone du dépôt, pour toujours.",
            "Stockez-le ailleurs (stockage de fichiers, Git LFS) et retirez-le du dépôt.",
        ),

        // ── DevOps ────────────────────────────────────────────────────────
        "dockerfile-latest-tag" => t(
            "Image Docker en version « latest »",
            "latest change sans prévenir : la même image de base peut produire un résultat différent d'une compilation à l'autre.",
            "Indiquez une version précise de l'image (node:20.11-alpine, par exemple).",
        ),
        "dockerfile-root-user" => t(
            "Conteneur exécuté en root",
            "Sans instruction USER, le conteneur tourne en administrateur : une faille dans l'application donne tous les droits dans le conteneur.",
            "Créez un utilisateur dédié et ajoutez USER à la fin du Dockerfile.",
        ),
        "dockerfile-missing-healthcheck" => t(
            "Pas de contrôle de santé du conteneur",
            "Sans HEALTHCHECK, l'orchestrateur ne voit pas qu'un conteneur est bloqué et ne le redémarre pas.",
            "Ajoutez une instruction HEALTHCHECK qui interroge l'application.",
        ),
        "dockerfile-hardcoded-secret" => t(
            "Secret écrit dans le Dockerfile",
            "Un secret passé par ENV ou ARG reste lisible dans l'image par toute personne qui la télécharge.",
            "Utilisez un secret de build (--secret) ou une variable d'environnement fournie au lancement.",
        ),
        "workflow-unpinned-action" => t(
            "Action GitHub non épinglée",
            "uses: owner/repo@v1 suit une étiquette modifiable : si l'action est compromise, votre pipeline exécute le nouveau code avec vos secrets.",
            "Épinglez l'action sur le SHA complet d'un commit.",
        ),
        "workflow-write-all-permissions" => t(
            "Pipeline avec tous les droits en écriture",
            "write-all donne au jeton du job bien plus de droits que nécessaire : une étape compromise peut modifier tout le dépôt.",
            "Déclarez uniquement les permissions nécessaires (contents: read, par exemple).",
        ),
        "workflow-secret-in-run" => t(
            "Secret affiché dans les journaux du pipeline",
            "Un echo d'un secret peut l'écrire dans les journaux du job, lisibles par d'autres personnes.",
            "N'affichez jamais un secret ; passez-le en variable d'environnement à la commande qui en a besoin.",
        ),
        "workflow-pull-request-target" => t(
            "Déclencheur pull_request_target",
            "Ce déclencheur donne accès aux secrets du dépôt même pour une pull request venant d'un fork inconnu.",
            "Utilisez pull_request, ou ne récupérez jamais le code de la PR dans ce job.",
        ),

        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_rules_have_french_texts() {
        let text = rule_text("sql-string-format").unwrap();
        assert!(text.title.starts_with("Requête SQL"));
        assert!(!text.why.is_empty() && !text.fix.is_empty());
        assert_eq!(rule_title("long-line"), "Ligne trop longue");
    }

    #[test]
    fn unknown_rules_fall_back_to_their_id() {
        assert_eq!(rule_title("ma-regle-perso"), "Ma regle perso");
    }

    /// Toutes les règles du moteur (catalogue intégré) ont un texte français.
    #[test]
    fn every_engine_rule_is_translated() {
        let missing: Vec<String> = herald_engine::dsl::builtin_rules()
            .into_iter()
            .chain(herald_engine::rules::extra_rules())
            .map(|rule| rule.id.to_string())
            .filter(|id| rule_text(id).is_none())
            .collect();
        assert!(
            missing.is_empty(),
            "règles sans texte français : {missing:?}"
        );
    }
}

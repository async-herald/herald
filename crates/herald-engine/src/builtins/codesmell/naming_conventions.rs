//! Per-language naming conventions. Deliberately conservative to avoid false
//! positives: only *unambiguous* violations are flagged. A single lowercase word
//! (`count`) is valid as both camelCase and snake_case, so it's never flagged.

use tree_sitter::Tree;

use crate::category::Category;
use crate::helpers::ast_query;
use crate::language::Language;
use crate::types::{FnRule, Issue, Severity};

pub const META: FnRule = FnRule {
    name: "naming-conventions",
    category: Category::Quality,
    check,
};

fn core(name: &str) -> &str {
    name.trim_matches('_')
}

fn is_dunder(name: &str) -> bool {
    name.starts_with("__") && name.ends_with("__")
}

fn has_internal_underscore(name: &str) -> bool {
    core(name).contains('_')
}

fn has_upper(name: &str) -> bool {
    name.chars().any(|c| c.is_ascii_uppercase())
}

fn starts_lower(name: &str) -> bool {
    core(name)
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase())
}

fn has_lower(name: &str) -> bool {
    name.chars().any(|c| c.is_ascii_lowercase())
}

/// `what` est un identifiant technique anglais (`"function"`, `"class"`…),
/// commun aux tables de règles par langage ci-dessous — traduit seulement au
/// point d'affichage plutôt que dans chacune de ces ~35 entrées.
fn what_fr(what: &str) -> &str {
    match what {
        "function" => "Fonction",
        "method" => "Méthode",
        "class" => "Classe",
        "interface" => "Interface",
        "enum" => "Énumération",
        "type" => "Type",
        "trait" => "Trait",
        "constant" => "Constante",
        "struct" => "Structure",
        "object" => "Objet",
        "protocol" => "Protocole",
        other => other,
    }
}

fn flag(name: &str, what: &str, want: &str, line: usize, column: usize) -> Issue {
    Issue {
        rule: META.name.to_string(),
        category: Category::Quality,
        severity: Severity::Warning,
        message: format!("{} '{name}' ne suit pas la convention {want}", what_fr(what)),
        line,
        column,
    }
}

/// Vrai si, DANS CE FICHIER, au moins deux tiers des noms capturés par `query`
/// utilisent déjà un underscore interne (snake_case) — un choix de style
/// assumé (ex: le cœur du compilateur Svelte, entièrement en snake_case) plutôt
/// que des fautes de frappe éparses. Se battre contre une convention locale
/// cohérente avec une convention "universelle" fixe ne fait que noyer le
/// rapport de bruit, sans rien qu'un humain va réellement corriger.
/// `total >= 4` : il faut un échantillon significatif, pas juste 1-2 noms.
fn snake_case_dominant(tree: Option<&Tree>, code: &str, language: Language, query: &'static str) -> bool {
    let mut total = 0usize;
    let mut snake = 0usize;
    ast_query(tree, code, language, query, |name, _line, _col| {
        if name != "_" && core(name).len() >= 2 {
            total += 1;
            if has_internal_underscore(name) {
                snake += 1;
            }
        }
        Vec::new()
    });
    total >= 4 && snake * 3 >= total * 2
}

pub fn check(code: &str, tree: Option<&Tree>, language: Language) -> Vec<Issue> {
    // (query, what, wanted, violates, convention_flexible). `convention_flexible`
    // n'est vrai que pour camelCase-vs-snake_case sur des fonctions/méthodes —
    // le seul cas où les deux styles sont des choix légitimes largement
    // répandus selon les projets (contrairement à PascalCase pour un type,
    // quasi universel, jamais une "alternative snake_case" assumée).
    type Rule = (&'static str, &'static str, &'static str, fn(&str) -> bool, bool);

    let snake_func: fn(&str) -> bool = |n| !is_dunder(n) && has_upper(n);
    let camel_func: fn(&str) -> bool = |n| has_internal_underscore(n);
    let pascal_type: fn(&str) -> bool = |n| starts_lower(n);
    let go_mixed: fn(&str) -> bool = |n| has_internal_underscore(n);
    let upper_const: fn(&str) -> bool = |n| has_lower(n);

    let rules: Vec<Rule> = match language {
        Language::Python => vec![
            ("(function_definition name: (identifier) @n)", "function", "snake_case", snake_func, false),
            ("(class_definition name: (identifier) @n)", "class", "PascalCase", pascal_type, false),
        ],
        Language::JavaScript => vec![
            ("(function_declaration name: (identifier) @n)", "function", "camelCase", camel_func, true),
            ("(method_definition name: (property_identifier) @n)", "method", "camelCase", camel_func, true),
            ("(class_declaration name: (identifier) @n)", "class", "PascalCase", pascal_type, false),
        ],
        Language::TypeScript => vec![
            ("(function_declaration name: (identifier) @n)", "function", "camelCase", camel_func, true),
            ("(method_definition name: (property_identifier) @n)", "method", "camelCase", camel_func, true),
            ("(class_declaration name: (type_identifier) @n)", "class", "PascalCase", pascal_type, false),
        ],
        Language::Java => vec![
            ("(method_declaration name: (identifier) @n)", "method", "camelCase", camel_func, true),
            ("(class_declaration name: (identifier) @n)", "class", "PascalCase", pascal_type, false),
            ("(interface_declaration name: (identifier) @n)", "interface", "PascalCase", pascal_type, false),
            ("(enum_declaration name: (identifier) @n)", "enum", "PascalCase", pascal_type, false),
        ],
        Language::Go => vec![
            ("(function_declaration name: (identifier) @n)", "function", "MixedCaps (no underscores)", go_mixed, true),
            ("(method_declaration name: (field_identifier) @n)", "method", "MixedCaps (no underscores)", go_mixed, true),
            ("(type_spec name: (type_identifier) @n)", "type", "MixedCaps (no underscores)", go_mixed, false),
        ],
        // PSR-1 / PSR-12. Plain (non-method) functions are deliberately left
        // out: PSR-1 doesn't mandate a case for them and snake_case functions
        // are pervasive in legacy PHP — flagging them would be noise.
        Language::Php => vec![
            ("(class_declaration name: (name) @n)", "class", "StudlyCaps", pascal_type, false),
            ("(interface_declaration name: (name) @n)", "interface", "StudlyCaps", pascal_type, false),
            ("(trait_declaration name: (name) @n)", "trait", "StudlyCaps", pascal_type, false),
            ("(enum_declaration name: (name) @n)", "enum", "StudlyCaps", pascal_type, false),
            ("(method_declaration name: (name) @n)", "method", "camelCase", camel_func, true),
            ("(const_element (name) @n)", "constant", "UPPER_SNAKE_CASE", upper_const, false),
        ],
        Language::CSharp => vec![
            ("(method_declaration name: (identifier) @n)", "method", "PascalCase", pascal_type, false),
            ("(class_declaration name: (identifier) @n)", "class", "PascalCase", pascal_type, false),
            ("(interface_declaration name: (identifier) @n)", "interface", "PascalCase", pascal_type, false),
            ("(struct_declaration name: (identifier) @n)", "struct", "PascalCase", pascal_type, false),
            ("(enum_declaration name: (identifier) @n)", "enum", "PascalCase", pascal_type, false),
        ],
        // Les classes/modules Ruby n'ont pas besoin d'être vérifiés : leur nom
        // DOIT être une `constant` (le lexer exige une majuscule initiale), donc
        // un nom en snake_case est déjà un rejet à l'analyse syntaxique.
        Language::Ruby => vec![
            ("(method name: (identifier) @n)", "method", "snake_case", snake_func, false),
            ("(singleton_method name: (identifier) @n)", "method", "snake_case", snake_func, false),
        ],
        Language::Kotlin => vec![
            ("(function_declaration name: (identifier) @n)", "function", "camelCase", camel_func, true),
            ("(class_declaration name: (identifier) @n)", "class", "PascalCase", pascal_type, false),
            ("(object_declaration name: (identifier) @n)", "object", "PascalCase", pascal_type, false),
        ],
        // Convention snake_case pour les fonctions C : quasi universelle (POSIX,
        // libc, noyau Linux). Les noms de struct/typedef ne le sont pas assez
        // (styles `_t`, PascalCase, préfixes Hongrois cohabitent) pour être
        // vérifiés sans bruit.
        Language::C => vec![(
            "(function_definition declarator: (function_declarator declarator: (identifier) @n))",
            "function", "snake_case", snake_func, false,
        )],
        // En C++, la casse des fonctions/méthodes varie trop selon les guides de
        // style (Google, LLVM, Qt…) pour être vérifiée sans bruit. Les noms de
        // classe/struct en PascalCase, eux, sont quasi universels.
        Language::Cpp => vec![
            ("(class_specifier name: (type_identifier) @n)", "class", "PascalCase", pascal_type, false),
            ("(struct_specifier name: (type_identifier) @n)", "struct", "PascalCase", pascal_type, false),
        ],
        Language::Swift => vec![
            ("(function_declaration name: (simple_identifier) @n)", "function", "camelCase", camel_func, true),
            ("(class_declaration name: (type_identifier) @n)", "type", "PascalCase", pascal_type, false),
            ("(protocol_declaration name: (type_identifier) @n)", "protocol", "PascalCase", pascal_type, false),
        ],
        Language::Lua => vec![(
            "(function_declaration name: (identifier) @n)",
            "function", "snake_case", snake_func, false,
        )],
        Language::Dart => vec![
            ("(function_signature name: (identifier) @n)", "function", "camelCase", camel_func, true),
            ("(class_declaration name: (identifier) @n)", "class", "PascalCase", pascal_type, false),
        ],
        Language::Rust => Vec::new(), // rustc already lints non_snake_case / non_camel_case
        // HTML/CSS n'ont pas d'identifiants de fonction/classe au sens de cette
        // règle (les conventions de nommage des sélecteurs CSS sont un autre
        // sujet, trop dépendant du style — BEM, kebab-case, etc.).
        _ => Vec::new(),
    };

    let mut issues = Vec::new();
    for (query, what, want, violates, flexible) in rules {
        if flexible && snake_case_dominant(tree, code, language, query) {
            continue;
        }
        issues.extend(ast_query(tree, code, language, query, |name, line, column| {
            if name == "_" || core(name).len() < 2 {
                return Vec::new();
            }
            if violates(name) {
                vec![flag(name, what, want, line, column)]
            } else {
                Vec::new()
            }
        }));
    }

    issues.sort_by_key(|i| (i.line, i.column));
    issues.dedup_by_key(|i| (i.line, i.column));
    issues
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::language::Language;
    use crate::language::Language::Php;

    fn flags(code: &str) -> Vec<String> {
        flags_lang(Php, code)
    }

    fn flags_lang(lang: Language, code: &str) -> Vec<String> {
        let mut p = tree_sitter::Parser::new();
        p.set_language(&lang.tree_sitter()).unwrap();
        let tree = p.parse(code, None).unwrap();
        check(code, Some(&tree), lang)
            .into_iter()
            .map(|i| i.message)
            .collect()
    }

    #[test]
    fn flags_psr_violations() {
        assert_eq!(flags("<?php class userController {}").len(), 1);
        assert_eq!(flags("<?php interface user_repo {}").len(), 1);
        assert_eq!(flags("<?php trait timestampable {}").len(), 1);
        assert_eq!(flags("<?php enum status {}").len(), 1);
        assert_eq!(flags("<?php class C { public function get_user() {} }").len(), 1);
        assert_eq!(flags("<?php class C { const maxItems = 1; }").len(), 1);
    }

    #[test]
    fn accepts_idiomatic_php() {
        assert!(flags("<?php class UserController {}").is_empty());
        assert!(flags("<?php interface UserRepository {}").is_empty());
        assert!(flags("<?php class C { public function getUser() {} }").is_empty());
        assert!(flags("<?php class C { const MAX_ITEMS = 1; }").is_empty());
        // Magic methods keep their underscores.
        assert!(flags("<?php class C { public function __construct() {} }").is_empty());
        assert!(flags("<?php class C { public function __toString() {} }").is_empty());
        // Plain functions are out of scope — legacy snake_case is not flagged.
        assert!(flags("<?php function get_user_by_id($id) {}").is_empty());
    }

    #[test]
    fn flags_csharp_violations() {
        assert_eq!(flags_lang(Language::CSharp, "class userAccount {}").len(), 1);
        assert_eq!(flags_lang(Language::CSharp, "interface repository {}").len(), 1);
        assert_eq!(
            flags_lang(Language::CSharp, "class C { void doThing() {} }").len(),
            1
        );
    }

    #[test]
    fn accepts_idiomatic_csharp() {
        assert!(flags_lang(Language::CSharp, "class UserAccount {}").is_empty());
        assert!(flags_lang(Language::CSharp, "interface IRepository {}").is_empty());
        assert!(flags_lang(Language::CSharp, "class C { void DoThing() {} }").is_empty());
    }

    #[test]
    fn flags_ruby_violations() {
        assert_eq!(flags_lang(Language::Ruby, "def getUser\nend").len(), 1);
        assert_eq!(
            flags_lang(Language::Ruby, "class C\n  def self.getUser\n  end\nend").len(),
            1
        );
    }

    #[test]
    fn accepts_idiomatic_ruby() {
        assert!(flags_lang(Language::Ruby, "def get_user\nend").is_empty());
        assert!(flags_lang(Language::Ruby, "class UserAccount\nend").is_empty());
        assert!(flags_lang(Language::Ruby, "module UserAccount\nend").is_empty());
    }

    /// Convention locale : un fichier où la majorité des fonctions sont déjà
    /// en snake_case (ex: le cœur du compilateur Svelte) ne doit pas être
    /// combattu comme si camelCase était la seule règle valable.
    #[test]
    fn accepts_snake_case_dominant_js_file() {
        let code = "\
            function create_sources() {}\n\
            function create_derived() {}\n\
            function compile_svelte() {}\n\
            function generate_report() {}\n\
            function read_profile() {}\n\
        ";
        assert!(flags_lang(Language::JavaScript, code).is_empty());
    }

    /// Contre-épreuve : un fichier très majoritairement camelCase avec UNE
    /// fonction isolée en snake_case reste flagué — ce n'est pas une
    /// convention locale, juste un écart isolé.
    #[test]
    fn flags_isolated_snake_case_in_camel_case_file() {
        let code = "\
            function getUser() {}\n\
            function setUser() {}\n\
            function loadProfile() {}\n\
            function saveProfile() {}\n\
            function fetch_data() {}\n\
        ";
        let issues = flags_lang(Language::JavaScript, code);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("fetch_data"));
    }

    #[test]
    fn flags_kotlin_violations() {
        assert_eq!(flags_lang(Language::Kotlin, "fun get_user() {}").len(), 1);
        assert_eq!(flags_lang(Language::Kotlin, "class userAccount {}").len(), 1);
    }

    #[test]
    fn accepts_idiomatic_kotlin() {
        assert!(flags_lang(Language::Kotlin, "fun getUser() {}").is_empty());
        assert!(flags_lang(Language::Kotlin, "class UserAccount {}").is_empty());
    }

    #[test]
    fn flags_c_violations() {
        assert_eq!(flags_lang(Language::C, "int getUser(void) { return 0; }").len(), 1);
    }

    #[test]
    fn accepts_idiomatic_c() {
        assert!(flags_lang(Language::C, "int get_user(void) { return 0; }").is_empty());
    }

    #[test]
    fn flags_cpp_violations() {
        assert_eq!(flags_lang(Language::Cpp, "class userAccount {};").len(), 1);
        assert_eq!(flags_lang(Language::Cpp, "struct user_account {};").len(), 1);
    }

    #[test]
    fn accepts_idiomatic_cpp() {
        assert!(flags_lang(Language::Cpp, "class UserAccount {};").is_empty());
        assert!(flags_lang(Language::Cpp, "struct UserAccount {};").is_empty());
        // Function casing is deliberately out of scope for C++ (too divergent).
        assert!(flags_lang(Language::Cpp, "int get_user() { return 0; }").is_empty());
    }

    #[test]
    fn flags_swift_violations() {
        assert_eq!(flags_lang(Language::Swift, "func get_user() {}").len(), 1);
        assert_eq!(flags_lang(Language::Swift, "class userAccount {}").len(), 1);
    }

    #[test]
    fn accepts_idiomatic_swift() {
        assert!(flags_lang(Language::Swift, "func getUser() {}").is_empty());
        assert!(flags_lang(Language::Swift, "class UserAccount {}").is_empty());
        assert!(flags_lang(Language::Swift, "protocol UserRepository {}").is_empty());
    }

    #[test]
    fn flags_lua_violations() {
        assert_eq!(flags_lang(Language::Lua, "function getUser() end").len(), 1);
    }

    #[test]
    fn accepts_idiomatic_lua() {
        assert!(flags_lang(Language::Lua, "function get_user() end").is_empty());
    }

    #[test]
    fn flags_dart_violations() {
        assert_eq!(flags_lang(Language::Dart, "void get_user() {}").len(), 1);
        assert_eq!(flags_lang(Language::Dart, "class userAccount {}").len(), 1);
    }

    #[test]
    fn accepts_idiomatic_dart() {
        assert!(flags_lang(Language::Dart, "void getUser() {}").is_empty());
        assert!(flags_lang(Language::Dart, "class UserAccount {}").is_empty());
    }
}

use tree_sitter::Language as TsLanguage;

use Language::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    JavaScript, TypeScript, Rust, Python, Java, Go, Php,
    CSharp, Ruby, Kotlin, C, Cpp, Swift, Lua, Dart,
}

type Entry = (Language, &'static [&'static str], &'static [&'static str], fn() -> TsLanguage);

#[rustfmt::skip]
const REGISTRY: &[Entry] = &[
    (JavaScript, &["js","jsx","mjs","cjs"],          &["javascript","node"],      || tree_sitter_javascript::LANGUAGE.into()),
    (TypeScript, &["ts","tsx"],                       &["typescript"],             || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
    (Rust,       &["rs"],                             &["rust"],                   || tree_sitter_rust::LANGUAGE.into()),
    (Python,     &["py"],                             &["python"],                 || tree_sitter_python::LANGUAGE.into()),
    (Go,         &["go"],                             &["golang"],                 || tree_sitter_go::LANGUAGE.into()),
    (Java,       &["java"],                           &[],                         || tree_sitter_java::LANGUAGE.into()),
    (Php,        &["php","phtml"],                    &[],                         || tree_sitter_php::LANGUAGE_PHP.into()),
    (CSharp,     &["cs","csx"],                       &["csharp","c#","c-sharp"],  || tree_sitter_c_sharp::LANGUAGE.into()),
    (Ruby,       &["rb","rake","gemspec"],            &["ruby"],                   || tree_sitter_ruby::LANGUAGE.into()),
    (Kotlin,     &["kt","kts"],                       &["kotlin"],                 || tree_sitter_kotlin_ng::LANGUAGE.into()),
    (C,          &["c","h"],                          &[],                         || tree_sitter_c::LANGUAGE.into()),
    (Cpp,        &["cpp","cc","cxx","hpp","hh","hxx"],&["c++"],                    || tree_sitter_cpp::LANGUAGE.into()),
    (Swift,      &["swift"],                          &[],                         || tree_sitter_swift::LANGUAGE.into()),
    (Lua,        &["lua"],                            &[],                         || tree_sitter_lua::LANGUAGE.into()),
    (Dart,       &["dart"],                           &[],                         || tree_sitter_dart::LANGUAGE.into()),
];

impl Language {
    fn entry(self) -> &'static Entry {
        REGISTRY.iter().find(|e| e.0 == self).expect("language missing from REGISTRY")
    }

    pub fn tree_sitter(self) -> TsLanguage {
        (self.entry().3)()
    }

    pub fn from_path(path: &str) -> Option<Self> {
        let ext = path.rsplit('.').next()?.to_lowercase();
        REGISTRY.iter().find(|e| e.1.contains(&ext.as_str())).map(|e| e.0)
    }

    pub fn from_name(name: &str) -> Option<Self> {
        let n = name.trim().to_lowercase();
        REGISTRY.iter()
            .find(|e| e.1.contains(&n.as_str()) || e.2.contains(&n.as_str()))
            .map(|e| e.0)
    }
}

#[cfg(test)]
mod tests {
    use super::Language::{self, *};

    #[test]
    fn from_name_handles_aliases_and_extensions() {
        assert_eq!(Language::from_name("javascript"), Some(JavaScript));
        assert_eq!(Language::from_name("node"), Some(JavaScript));
        assert_eq!(Language::from_name("golang"), Some(Go));
        assert_eq!(Language::from_name("c#"), Some(CSharp));
        assert_eq!(Language::from_name("c++"), Some(Cpp));
        assert_eq!(Language::from_name("  TS "), Some(TypeScript));
        assert_eq!(Language::from_name("rs"), Some(Rust));
        assert_eq!(Language::from_name("cobol"), None);
    }

    #[test]
    fn from_path_uses_extension() {
        assert_eq!(Language::from_path("src/main.rs"), Some(Rust));
        assert_eq!(Language::from_path("a/App.tsx"), Some(TypeScript));
        assert_eq!(Language::from_path("noext"), None);
    }
}

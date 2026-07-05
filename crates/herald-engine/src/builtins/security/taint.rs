//! Analyse de taint intra-fonction (dataflow léger) : suit une donnée non
//! fiable (source) depuis son entrée, à travers les affectations de variables,
//! jusqu'à une opération dangereuse (sink). Couvre les flux OWASP les plus
//! importants : injection de commande, de code (eval), SQL et XSS.
//!
//! Portée : par fonction (les variables ne « fuient » pas d'une fonction à
//! l'autre). Propagation par fixpoint sur les affectations. Un appel à un
//! sanitizer connu sur la ligne neutralise le flux.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use tree_sitter::{Node, Tree};

use crate::category::Category;
use crate::language::Language;
use crate::types::{Issue, Severity};

struct LangSpec {
    sink: &'static Regex,
    source: &'static Regex,
    sani: &'static Regex,
}

struct Cfg {
    rule: &'static str,
    msg: &'static str,
    /// Le flux ne compte que s'il passe par une construction de chaîne
    /// (concat / template / format). Réservé à SQL pour écarter les requêtes
    /// paramétrées (input passé en paramètre lié, pas concaténé).
    require_concat: bool,
    js: Option<LangSpec>,
    py: Option<LangSpec>,
}

// ───────────────────────── briques communes ─────────────────────────

static ASSIGN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:const\s+|let\s+|var\s+)?([A-Za-z_$][\w$]*)\s*=\s*([^=].*)$").unwrap()
});
// Marqueurs de construction de chaîne (injection par concaténation).
static CONCAT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\+|\$\{|\.format\s*\(|%\s|f["']"#).unwrap());

static JS_SOURCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:req|request|ctx|context)\s*\.\s*(?:body|query|params|headers|cookies)\b|\bprocess\.argv\b|\blocation\.(?:search|hash|href)\b|\bdocument\.(?:URL|cookie|referrer)\b|\bwindow\.name\b",
    )
    .unwrap()
});
static PY_SOURCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\brequest\.(?:args|form|json|data|values|GET|POST|headers|cookies|files)\b|\bsys\.argv\b|\binput\s*\(|\bflask\.request\b",
    )
    .unwrap()
});
static JS_SANI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(?:parseInt|parseFloat|Number|encodeURIComponent|encodeURI|escapeHtml|sanitize\w*|validator\.\w+|DOMPurify|zod\.|Joi\.|yup\.)\s*\(",
    )
    .unwrap()
});
static PY_SANI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:int|float|shlex\.quote|bleach\.clean|markupsafe\.escape|re\.escape)\s*\(")
        .unwrap()
});

// ───────────────────────── sinks par flux ─────────────────────────

static CMD_JS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:child_process\s*\.\s*)?(?:exec|execSync|execFile|execFileSync|spawn|spawnSync)\s*\(").unwrap()
});
static CMD_PY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bos\.(?:system|popen)\s*\(|\bsubprocess\.(?:call|run|Popen|check_output|check_call|getoutput)\s*\(").unwrap()
});
static CODE_JS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\beval\s*\(|\bnew\s+Function\s*\(").unwrap());
static CODE_PY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\beval\s*\(|\bexec\s*\(|\b__import__\s*\(").unwrap());
static XSS_JS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\.\s*innerHTML\s*=|\.\s*outerHTML\s*=|\bdocument\.write(?:ln)?\s*\(|\binsertAdjacentHTML\s*\(").unwrap()
});
static SQL_JS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.\s*(?:query|execute|exec|raw)\s*\(").unwrap());
static SQL_PY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.\s*(?:execute|executemany|raw)\s*\(").unwrap());

// ───────────────────────── moteur ─────────────────────────

fn ident_refs(text: &str, set: &HashSet<String>) -> bool {
    if set.is_empty() {
        return false;
    }
    let b = text.as_bytes();
    let (n, mut i) = (b.len(), 0);
    while i < n {
        let c = b[i];
        if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
            let start = i;
            i += 1;
            while i < n && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'$') {
                i += 1;
            }
            if set.contains(&text[start..i]) {
                return true;
            }
        } else {
            i += 1;
        }
    }
    false
}

fn is_fn(kind: &str, lang: Language) -> bool {
    match lang {
        Language::Python => kind == "function_definition",
        _ => matches!(
            kind,
            "function_declaration"
                | "function_expression"
                | "arrow_function"
                | "method_definition"
                | "generator_function_declaration"
                | "function"
        ),
    }
}

fn collect_fns<'a>(node: Node<'a>, lang: Language, out: &mut Vec<Node<'a>>) {
    if is_fn(node.kind(), lang) {
        out.push(node);
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        collect_fns(child, lang, out);
    }
}

fn scan_scope(text: &str, base_row: usize, spec: &LangSpec, cfg: &Cfg, out: &mut Vec<Issue>) {
    let mut tainted: HashSet<String> = HashSet::new();
    let mut tainted_c: HashSet<String> = HashSet::new();

    // Fixpoint : propage la souillure à travers les affectations.
    loop {
        let mut changed = false;
        for line in text.lines() {
            if let Some(c) = ASSIGN.captures(line) {
                let var = c.get(1).unwrap().as_str();
                let rhs = c.get(2).unwrap().as_str();
                if spec.sani.is_match(rhs) {
                    continue;
                }
                let src = spec.source.is_match(rhs);
                let reft = ident_refs(rhs, &tainted);
                if (src || reft) && !tainted.contains(var) {
                    tainted.insert(var.to_string());
                    changed = true;
                }
                let refc = ident_refs(rhs, &tainted_c);
                let concat_flow = (src || reft) && CONCAT.is_match(rhs);
                if (concat_flow || refc) && !tainted_c.contains(var) {
                    tainted_c.insert(var.to_string());
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    let set = if cfg.require_concat { &tainted_c } else { &tainted };
    for (i, line) in text.lines().enumerate() {
        let Some(m) = spec.sink.find(line) else {
            continue;
        };
        if spec.sani.is_match(line) {
            continue;
        }
        let direct = spec.source.is_match(line) && (!cfg.require_concat || CONCAT.is_match(line));
        if direct || ident_refs(line, set) {
            out.push(Issue {
                rule: cfg.rule.to_string(),
                category: Category::Security,
                severity: Severity::Error,
                message: cfg.msg.to_string(),
                line: base_row + i + 1,
                column: m.start() + 1,
            });
        }
    }
}

fn run(code: &str, tree: Option<&Tree>, lang: Language, cfg: &Cfg) -> Vec<Issue> {
    let spec = match lang {
        Language::JavaScript | Language::TypeScript => cfg.js.as_ref(),
        Language::Python => cfg.py.as_ref(),
        _ => None,
    };
    let (Some(spec), Some(tree)) = (spec, tree) else {
        return Vec::new();
    };

    let root = tree.root_node();
    let mut fns = Vec::new();
    collect_fns(root, lang, &mut fns);

    let mut out = Vec::new();
    let bytes = code.as_bytes();
    if fns.is_empty() {
        // Pas de fonction : tout le fichier est un seul scope.
        scan_scope(code, 0, spec, cfg, &mut out);
    } else {
        for f in fns {
            let text = f.utf8_text(bytes).unwrap_or("");
            scan_scope(text, f.start_position().row, spec, cfg, &mut out);
        }
    }
    out.sort_by_key(|i| (i.line, i.column));
    out.dedup_by_key(|i| (i.line, i.column));
    out
}

// ───────────────────────── points d'entrée ─────────────────────────

pub fn check_command_injection(code: &str, tree: Option<&Tree>, lang: Language) -> Vec<Issue> {
    run(code, tree, lang, &Cfg {
        rule: "taint-command-injection",
        msg: "Injection de commande : une entrée non fiable atteint exec/spawn/os.system — un attaquant peut exécuter des commandes arbitraires ; évite le shell, passe les arguments en tableau et valide l'entrée",
        require_concat: false,
        js: Some(LangSpec { sink: &CMD_JS, source: &JS_SOURCE, sani: &JS_SANI }),
        py: Some(LangSpec { sink: &CMD_PY, source: &PY_SOURCE, sani: &PY_SANI }),
    })
}

pub fn check_code_injection(code: &str, tree: Option<&Tree>, lang: Language) -> Vec<Issue> {
    run(code, tree, lang, &Cfg {
        rule: "taint-code-injection",
        msg: "Injection de code : une entrée non fiable atteint eval/Function/exec — exécution de code arbitraire ; n'évalue jamais d'entrée, utilise un parseur (JSON.parse) ou une table de dispatch",
        require_concat: false,
        js: Some(LangSpec { sink: &CODE_JS, source: &JS_SOURCE, sani: &JS_SANI }),
        py: Some(LangSpec { sink: &CODE_PY, source: &PY_SOURCE, sani: &PY_SANI }),
    })
}

pub fn check_xss(code: &str, tree: Option<&Tree>, lang: Language) -> Vec<Issue> {
    run(code, tree, lang, &Cfg {
        rule: "taint-xss",
        msg: "XSS : une entrée non fiable atteint innerHTML/document.write — injection de script dans la page ; utilise textContent ou échappe/assainis (DOMPurify)",
        require_concat: false,
        js: Some(LangSpec { sink: &XSS_JS, source: &JS_SOURCE, sani: &JS_SANI }),
        py: None,
    })
}

pub fn check_sql_injection(code: &str, tree: Option<&Tree>, lang: Language) -> Vec<Issue> {
    run(code, tree, lang, &Cfg {
        rule: "taint-sql-injection",
        msg: "Injection SQL : une entrée non fiable est concaténée dans une requête puis exécutée — utilise des requêtes paramétrées (placeholders), jamais de concaténation",
        require_concat: true,
        js: Some(LangSpec { sink: &SQL_JS, source: &JS_SOURCE, sani: &JS_SANI }),
        py: Some(LangSpec { sink: &SQL_PY, source: &PY_SOURCE, sani: &PY_SANI }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::Language;

    fn parse(code: &str, lang: Language) -> Tree {
        let mut p = tree_sitter::Parser::new();
        p.set_language(&lang.tree_sitter()).unwrap();
        p.parse(code, None).unwrap()
    }

    fn n(code: &str, lang: Language, f: fn(&str, Option<&Tree>, Language) -> Vec<Issue>) -> usize {
        let t = parse(code, lang);
        f(code, Some(&t), lang).len()
    }

    #[test]
    fn command_injection_direct_and_via_var() {
        let js = Language::JavaScript;
        // direct : source sur la ligne du sink
        assert_eq!(
            n("function h(req){\n  exec(req.body.cmd)\n}", js, check_command_injection),
            1
        );
        // via variable (vraie propagation)
        assert_eq!(
            n("function h(req){\n  const c = req.query.c\n  exec(c)\n}", js, check_command_injection),
            1
        );
        // multi-hop a -> b -> sink
        assert_eq!(
            n("function h(req){\n  const a = req.body.x\n  const b = a\n  execSync(b)\n}", js, check_command_injection),
            1
        );
    }

    #[test]
    fn command_injection_sanitized_and_benign_are_clean() {
        let js = Language::JavaScript;
        // assaini
        assert_eq!(
            n("function h(req){\n  exec(parseInt(req.body.n))\n}", js, check_command_injection),
            0
        );
        // entrée constante
        assert_eq!(n("function h(){\n  exec('ls -la')\n}", js, check_command_injection), 0);
        // source jamais reliée au sink
        assert_eq!(
            n("function h(req){\n  const x = req.body.x\n  exec('git status')\n}", js, check_command_injection),
            0
        );
    }

    #[test]
    fn code_injection_eval() {
        let js = Language::JavaScript;
        assert_eq!(n("function h(req){\n  eval(req.query.code)\n}", js, check_code_injection), 1);
        assert_eq!(n("function h(){\n  eval('1+1')\n}", js, check_code_injection), 0);
    }

    #[test]
    fn xss_innerhtml() {
        let js = Language::JavaScript;
        assert_eq!(
            n("function h(req){\n  el.innerHTML = req.body.html\n}", js, check_xss),
            1
        );
        assert_eq!(
            n("function h(req){\n  const s = req.query.s\n  document.write(s)\n}", js, check_xss),
            1
        );
        assert_eq!(n("function h(){\n  el.innerHTML = '<b>ok</b>'\n}", js, check_xss), 0);
    }

    #[test]
    fn sql_concat_flagged_parameterized_clean() {
        let js = Language::JavaScript;
        // concaténation via variable → injection
        assert_eq!(
            n("function h(req){\n  const q = 'SELECT * FROM u WHERE id=' + req.params.id\n  db.query(q)\n}", js, check_sql_injection),
            1
        );
        // requête paramétrée → propre (input passé en paramètre lié)
        assert_eq!(
            n("function h(req){\n  db.query('SELECT * FROM u WHERE id=?', [req.params.id])\n}", js, check_sql_injection),
            0
        );
    }

    #[test]
    fn python_command_injection() {
        let py = Language::Python;
        assert_eq!(
            n("def h(request):\n    os.system(request.args.get('c'))\n", py, check_command_injection),
            1
        );
        assert_eq!(
            n("def h(request):\n    cmd = request.form['c']\n    subprocess.run(cmd)\n", py, check_command_injection),
            1
        );
    }

    #[test]
    fn scopes_do_not_bleed_between_functions() {
        let js = Language::JavaScript;
        // `x` souillé dans f, mais exec(x) est dans g avec un x propre → pas de flag
        let code = "function f(req){\n  const x = req.body.x\n}\nfunction g(){\n  const x = 'safe'\n  exec(x)\n}";
        assert_eq!(n(code, js, check_command_injection), 0);
    }
}

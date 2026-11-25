//! Vérification d'une règle `.herald` dans l'éditeur de l'app : la source se
//! lit-elle, son identifiant est-il libre, ses tests passent-ils ?

use crate::model::{Category, Severity};

/// Modèle d'une nouvelle règle, commenté pour quelqu'un qui découvre le format.
pub const TEMPLATE: &str = r#"// Une règle repère une forme de code. Écrivez la forme avec des $VARIABLES
// pour les trous, affinez avec « when », et prouvez-la avec « tests ».
rule ma-regle {
  meta {
    type: security
    severity: warning
    message: "Ce que la personne lira dans le rapport"
  }
  match js, ts: console.log($X)
  when $X is user-input
  tests {
    js {
      "console.log(req.body.password)" => fail
      "console.log('démarré')" => pass
    }
  }
}
"#;

/// Ce que la règle déclare dans son bloc `meta`.
#[derive(Clone, Debug, PartialEq)]
pub struct RuleSummary {
    pub id: String,
    pub category: Option<Category>,
    pub severity: Severity,
    pub message: String,
}

/// Un cas du bloc `tests` et son résultat.
#[derive(Clone, Debug, PartialEq)]
pub struct TestResult {
    /// Langage (`js`, `py`…) ou cible (`path`…).
    pub target: String,
    pub code: String,
    /// `true` : le cas doit être relevé (`=> fail`).
    pub expect_flag: bool,
    pub passed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuleCheck {
    /// `Err` : message à afficher (lecture impossible, identifiant pris…).
    pub summary: Result<RuleSummary, String>,
    pub tests: Vec<TestResult>,
}

impl RuleCheck {
    /// Une règle s'enregistre dès qu'elle se lit et que son identifiant est libre.
    pub fn can_save(&self) -> bool {
        self.summary.is_ok()
    }

    pub fn failed_tests(&self) -> usize {
        self.tests.iter().filter(|t| !t.passed).count()
    }
}

/// Vérifie `source`. `taken(id)` : l'identifiant appartient déjà à une autre
/// règle (officielle, ou personnelle autre que celle qu'on modifie).
pub fn check(source: &str, taken: impl Fn(&str) -> bool) -> RuleCheck {
    let meta = match herald_engine::dsl::parse_meta(source) {
        Ok(meta) => meta,
        Err(message) => {
            return RuleCheck {
                summary: Err(format!("La règle ne se lit pas : {message}")),
                tests: Vec::new(),
            };
        }
    };
    let tests = herald_engine::dsl::run_source_examples(source)
        .unwrap_or_default()
        .into_iter()
        .map(|outcome| TestResult {
            passed: outcome.passed(),
            target: outcome.target.to_ascii_lowercase(),
            code: outcome.value,
            expect_flag: outcome.expected_flag,
        })
        .collect();
    let summary = if taken(&meta.id) {
        Err(format!(
            "L'identifiant « {} » est déjà utilisé par une autre règle. Choisissez-en un autre.",
            meta.id
        ))
    } else {
        Ok(RuleSummary {
            category: Category::from_engine(&meta.category),
            severity: Severity::from_engine(&meta.severity),
            id: meta.id,
            message: meta.message,
        })
    };
    RuleCheck { summary, tests }
}

#[cfg(test)]
mod tests {
    use super::{TEMPLATE, check};
    use crate::model::Severity;

    #[test]
    fn the_template_is_a_valid_rule_whose_tests_pass() {
        let result = check(TEMPLATE, |_| false);
        let summary = result.summary.as_ref().expect("le modèle se lit");
        assert_eq!(summary.id, "ma-regle");
        assert_eq!(summary.severity, Severity::Warning);
        assert_eq!(result.tests.len(), 2);
        assert_eq!(result.failed_tests(), 0, "{:?}", result.tests);
    }

    #[test]
    fn a_taken_identifier_cannot_be_saved() {
        let result = check(TEMPLATE, |id| id == "ma-regle");
        assert!(!result.can_save());
        assert!(result.summary.unwrap_err().contains("ma-regle"));
    }

    #[test]
    fn an_unreadable_rule_explains_why() {
        let result = check("rule {", |_| false);
        assert!(!result.can_save());
        assert!(
            result
                .summary
                .unwrap_err()
                .starts_with("La règle ne se lit pas")
        );
    }

    #[test]
    fn a_failing_test_is_counted_but_does_not_block_saving() {
        let wrong = TEMPLATE.replace(
            "\"console.log('démarré')\" => pass",
            "\"console.log('démarré')\" => fail",
        );
        let result = check(&wrong, |_| false);
        assert!(result.can_save());
        assert_eq!(result.failed_tests(), 1);
    }
}

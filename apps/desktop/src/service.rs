//! Services en ligne de l'app, version locale : aucun. Les règles viennent du
//! catalogue intégré au moteur et des règles de l'app (`rules/`).

/// Catalogue de règles publié en ligne (JSON) ; `None` : catalogue intégré.
pub fn official_rules_json() -> Option<String> {
    None
}

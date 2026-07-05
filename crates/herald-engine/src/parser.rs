
use tree_sitter::{Parser, Tree};

use crate::language::Language;

pub fn parse(code: &str, lang: Language) -> Option<Tree> {
    let mut parser = Parser::new();
    parser.set_language(&lang.tree_sitter()).ok()?;
    parser.parse(code, None)
}

//! Built-in rule *mechanisms* routed by the `.herald` DSL:
//! - `security`/`codesmell` analyzers (FnRule `check` functions)
//! - metric counters (`codesmell::metrics`, `max_file_lines`, `long_line`)
//!
//! Rule *declarations* live in crates/herald-engine/rules/*.herald.

pub mod architecture;
pub mod codesmell;
pub mod security;

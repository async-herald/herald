//! Interface GPUI : la vue racine [`HeraldApp`] et ses écrans.

mod alerts;
mod analysis;
mod analysis_run;
mod app;
mod components;
mod dependencies;
mod dependency_modal;
mod false_positive_modal;
mod general;
mod home;
mod ide;
mod new_project;
mod overview;
mod patterns;
mod plan;
mod resource_monitor;
mod rule_editor;
mod rules;
mod settings;
mod sidebar;
// Version locale : ni compte, ni quota, ni mise à jour intégrée.
#[path = "local.rs"]
mod service;

pub use app::HeraldApp;
pub use components::bind_input_keys;

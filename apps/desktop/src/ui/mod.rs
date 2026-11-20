//! Interface GPUI : la vue racine [`HeraldApp`] et ses écrans.

mod alerts;
mod analysis;
mod analysis_run;
mod app;
mod code_editor;
mod components;
mod dependencies;
mod home;
mod ide;
mod new_project;
mod overview;
mod plan;
mod resource_monitor;
mod review;
mod rules;
mod settings;
mod sidebar;
// Version locale : ni compte, ni quota, ni mise à jour intégrée.
#[path = "local.rs"]
mod service;

pub use app::HeraldApp;
pub use components::bind_input_keys;

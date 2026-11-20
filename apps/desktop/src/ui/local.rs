//! Version locale de l'app : ni compte, ni quota, ni mise à jour intégrée.
//! L'app s'ouvre directement sur les projets ; tout se passe sur la machine.

use gpui::{AnyElement, Window, div, prelude::*};

use super::analysis_run::RunError;
use super::app::HeraldApp;
use crate::model::Usage;
use crate::theme::{Theme, metrics};

/// Pas de quota en version locale : aucune valeur possible.
pub(super) enum Quota {}

/// Réservation et enregistrement d'une analyse : rien à faire en local.
#[derive(Clone)]
pub(super) struct AnalysisService;

impl AnalysisService {
    pub(super) fn reserve(&self) -> Result<Option<Quota>, RunError> {
        Ok(None)
    }

    pub(super) fn record(
        &self,
        _project_name: String,
        _score: u8,
        _local_id: Option<i64>,
    ) -> Option<Quota> {
        None
    }
}

impl HeraldApp {
    pub(super) fn start_service(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.dev_autostart(cx);
    }

    pub(super) fn usage(&self) -> Option<Usage> {
        None
    }

    pub(super) fn usage_mut(&mut self) -> Option<&mut Usage> {
        None
    }

    pub(super) fn apply_quota(&mut self, quota: Quota) {
        match quota {}
    }

    pub(super) fn analysis_service(&self) -> AnalysisService {
        AnalysisService
    }

    pub(super) fn render_gate(
        &self,
        _theme: Theme,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        None
    }

    pub(super) fn render_update_banner(
        &self,
        _theme: Theme,
        _cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        None
    }

    pub(super) fn render_update_required(
        &self,
        _theme: Theme,
        _cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        None
    }

    /// Version de l'app en bas à droite.
    pub(super) fn render_version(&self, theme: Theme, _cx: &mut Context<Self>) -> AnyElement {
        div()
            .absolute()
            .bottom(metrics::SPACE_2)
            .right(metrics::SPACE_3)
            .text_size(metrics::TEXT_XS)
            .text_color(theme.ink_soft)
            .child(format!("Herald {}", crate::VERSION))
            .into_any_element()
    }

    pub(super) fn render_account_row(
        &self,
        _theme: Theme,
        _cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        None
    }

    pub(super) fn quota_note(&self) -> Option<&'static str> {
        None
    }

    pub(super) fn prepare_account_settings(&mut self, _cx: &mut Context<Self>) {}

    pub(super) fn render_account_section(
        &self,
        _theme: Theme,
        _cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        None
    }
}

use gpui::{
    App, ClickEvent, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder,
};

use super::{Icon, IconName};
use crate::theme::{Theme, metrics};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonVariant {
    /// Or plein : une seule action principale par écran.
    Primary,
    /// Blanc bordé : actions secondaires.
    Default,
    /// Texte seul : actions tertiaires et liens.
    Ghost,
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    icon: Option<IconName>,
    variant: ButtonVariant,
    theme: Theme,
    full_width: bool,
    large: bool,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, theme: Theme) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            variant: ButtonVariant::Default,
            theme,
            full_width: false,
            large: false,
            on_click: None,
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Bouton d'appel principal, plus grand (action phare d'un écran).
    pub fn large(mut self) -> Self {
        self.large = true;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let fg = match self.variant {
            ButtonVariant::Primary => theme.on_accent,
            ButtonVariant::Default => theme.ink,
            ButtonVariant::Ghost => theme.accent_text,
        };

        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .gap(metrics::SPACE_2)
            .h(metrics::CONTROL_H)
            .px(metrics::SPACE_3)
            .rounded(metrics::RADIUS)
            .text_size(metrics::TEXT_BASE)
            .font_weight(FontWeight::MEDIUM)
            .text_color(fg)
            .cursor_pointer()
            .map(|this| match self.variant {
                ButtonVariant::Primary => this
                    .bg(theme.accent)
                    .hover(move |style| style.bg(theme.accent.opacity(0.85))),
                ButtonVariant::Default => this
                    .bg(theme.bg_surface)
                    .border_1()
                    .border_color(theme.border)
                    .hover(move |style| style.bg(theme.bg_hover)),
                ButtonVariant::Ghost => this
                    .px(metrics::SPACE_2)
                    .hover(move |style| style.bg(theme.bg_hover)),
            })
            .when(self.full_width, |this| {
                this.w_full()
                    .justify_center()
                    .h(metrics::CONTROL_H + metrics::SPACE_2)
            })
            .when(self.large, |this| {
                this.h(metrics::CONTROL_H + metrics::SPACE_3)
                    .px(metrics::SPACE_5)
                    .rounded(metrics::RADIUS_LG)
                    .text_size(metrics::TEXT_MD)
                    .font_weight(FontWeight::SEMIBOLD)
                    .shadow_sm()
            })
            .when_some(self.icon, |this, icon| {
                this.child(Icon::new(icon, fg).size(metrics::ICON_SM))
            })
            .child(self.label)
            .when_some(self.on_click, |this, handler| this.on_click(handler))
    }
}

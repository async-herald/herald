use gpui::{App, Hsla, IntoElement, ParentElement, RenderOnce, Styled, Window, div, relative};

use crate::theme::{Theme, metrics};

/// Barre fine remplie à `ratio` (0.0 à 1.0).
#[derive(IntoElement)]
pub struct ProgressBar {
    ratio: f32,
    fill: Hsla,
    track: Hsla,
}

impl ProgressBar {
    pub fn new(ratio: f32, theme: Theme) -> Self {
        Self {
            ratio: ratio.clamp(0., 1.),
            fill: theme.accent,
            track: theme.track,
        }
    }

    pub fn fill(mut self, fill: Hsla) -> Self {
        self.fill = fill;
        self
    }
}

impl RenderOnce for ProgressBar {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .w_full()
            .h(metrics::BAR_H)
            .rounded_full()
            .bg(self.track)
            .child(
                div()
                    .h_full()
                    .w(relative(self.ratio))
                    .rounded_full()
                    .bg(self.fill),
            )
    }
}

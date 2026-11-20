use gpui::{App, IntoElement, ObjectFit, Pixels, RenderOnce, Styled, StyledImage, Window, img};

/// L'ange Herald (barre latérale et écran vide uniquement).
#[derive(IntoElement)]
pub struct Logo {
    size: Pixels,
}

impl Logo {
    pub fn new(size: Pixels) -> Self {
        Self { size }
    }
}

impl RenderOnce for Logo {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        img("brand/angel.png")
            .flex_none()
            .size(self.size)
            .object_fit(ObjectFit::Contain)
    }
}

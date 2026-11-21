//! Ornements du héraut (style « ange » de la marque) : ailes autour des titres,
//! auréole au-dessus des meilleures notes. Tracés repris de l'emblème
//! (`assets/emblem`), teintés de l'or du thème.

use gpui::{AnyElement, Div, FontWeight, SharedString, div, img, prelude::*, px, svg};

use crate::theme::{Theme, metrics};

/// Bandeau « Le héraut annonce » : l'ange, des rayons pâles, un titre, une
/// phrase d'explication et une action facultative à droite.
pub fn herald_banner(
    caption: &str,
    headline: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    action: Option<AnyElement>,
    theme: Theme,
) -> Div {
    herald_banner_with(caption, headline, detail, None, action, theme)
}

/// Bandeau du héraut avec, sous le texte, un élément de plus (la barre de
/// progression pendant une analyse).
pub fn herald_banner_with(
    caption: &str,
    headline: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    below: Option<AnyElement>,
    action: Option<AnyElement>,
    theme: Theme,
) -> Div {
    div()
        .relative()
        .overflow_hidden()
        .flex()
        .items_center()
        .gap(metrics::SPACE_4)
        .px(metrics::SPACE_5)
        .py(metrics::SPACE_3)
        .rounded(metrics::RADIUS_LG)
        .border_1()
        .border_color(theme.herald_border)
        .bg(theme.herald_bg)
        // Rayons de l'emblème, très pâles, à droite.
        .child(
            svg()
                .path("emblem/rays.svg")
                .absolute()
                .right(px(-60.))
                .top(px(-90.))
                .size(px(260.))
                .text_color(theme.halo.opacity(0.10)),
        )
        .child(img("brand/angel.png").flex_none().size(px(56.)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(metrics::TEXT_XS)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.herald_text)
                        .child(caption.to_uppercase()),
                )
                .child(
                    div()
                        .text_size(metrics::TEXT_MD)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(headline.into()),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(metrics::TEXT_SM)
                        .text_color(theme.ink_muted)
                        .child(detail.into()),
                )
                .children(below.map(|below| div().pt(metrics::SPACE_2).child(below))),
        )
        .children(action)
}

//! Emblème animé du Herald, repris de `heraldapp-rs/public/herald-pro-animated.svg`.
//!
//! GPUI ne joue pas les animations CSS d'un SVG : l'emblème est découpé en calques
//! (`assets/emblem/`) animés ici avec les mêmes durées que l'original.
//! - flottement de l'ensemble (3 s) ;
//! - battement des quatre ailes autour de leur racine (1,2 s, aller-retour) ;
//! - pulsation du halo (3,6 s) ;
//! - regard qui se déplace (5,4 s) et clignement (9 s).

use std::f32::consts::TAU;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, App, IntoElement, ParentElement, Pixels, RenderOnce, Styled,
    Transformation, Window, div, img, radians, svg,
};

use crate::theme::Theme;

/// Taille du repère de l'original (`viewBox="-2 -2 104 104"`). Les calques déclarent
/// `width="1040"` : GPUI rastérise un SVG à sa taille déclarée, une petite taille
/// serait pixelisée une fois agrandie.
const VIEWBOX: f32 = 104.;
/// Les ailes sont dessinées dans un carré de 72 unités centré sur leur racine.
const WING_BOX: f32 = 72.;
const FLOAT_AMPLITUDE: f32 = 2.5;
const WING_ANGLE_DEG: f32 = 15.;
const PUPIL_TRAVEL: f32 = 2.;

/// (nom de fichier, racine x, racine y, sens de rotation, aile basse)
const WINGS: [(&str, f32, f32, f32, bool); 4] = [
    ("tl", 42., 44., -1., false),
    ("tr", 58., 44., 1., false),
    ("bl", 42., 56., 1., true),
    ("br", 58., 56., -1., true),
];

/// Va-et-vient doux entre 0 et 1 sur un cycle (équivalent d'un `ease-in-out` alterné).
fn wave(delta: f32) -> f32 {
    (1. - (TAU * delta).cos()) / 2.
}

/// Courbe de lissage entre deux paliers.
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// Déplacement horizontal de la pupille (`hp-pupil-move`), en unités du repère.
fn pupil_offset(delta: f32) -> f32 {
    let between = |from: f32, to: f32, start: f32, end: f32| {
        from + (to - from) * smooth((delta - start) / (end - start))
    };
    match delta {
        d if d < 0.20 => 0.,
        d if d < 0.35 => between(0., PUPIL_TRAVEL, 0.20, 0.35),
        d if d < 0.50 => PUPIL_TRAVEL,
        d if d < 0.62 => between(PUPIL_TRAVEL, -PUPIL_TRAVEL, 0.50, 0.62),
        d if d < 0.80 => -PUPIL_TRAVEL,
        d if d < 0.92 => between(-PUPIL_TRAVEL, 0., 0.80, 0.92),
        _ => 0.,
    }
}

/// Opacité de l'œil pendant le clignement (`hp-eye-blink`, 97 % → 99 % du cycle).
fn blink_opacity(delta: f32) -> f32 {
    if (0.97..0.99).contains(&delta) {
        0.1
    } else {
        1.
    }
}

#[derive(IntoElement)]
pub struct HeraldEmblem {
    size: Pixels,
    theme: Theme,
}

impl HeraldEmblem {
    pub fn new(size: Pixels, theme: Theme) -> Self {
        Self { size, theme }
    }
}

impl RenderOnce for HeraldEmblem {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let Self { size, theme } = self;
        let unit = size / VIEWBOX;
        let layer = |path: &'static str| img(path).absolute().top_0().left_0().size(size);

        let wings = WINGS
            .into_iter()
            .flat_map(move |(name, root_x, root_y, sign, lower)| {
                let fill = if lower {
                    theme.emblem_wing_deep
                } else {
                    theme.emblem_wing
                };
                // Origine du repère à -2 : la racine se retrouve au centre de l'élément.
                let left = unit * (root_x - WING_BOX / 2. + 2.);
                let top = unit * (root_y - WING_BOX / 2. + 2.);
                [("fill", fill), ("line", theme.emblem_line)].map(|(part, color)| {
                    svg()
                        .path(format!("emblem/wing-{name}-{part}.svg"))
                        .absolute()
                        .left(left)
                        .top(top)
                        .size(unit * WING_BOX)
                        .text_color(color)
                        .with_animation(
                            gpui::SharedString::from(format!("emblem-wing-{name}-{part}")),
                            Animation::new(Duration::from_millis(2_400)).repeat(),
                            move |wing, delta| {
                                let angle = sign * WING_ANGLE_DEG * wave(delta);
                                wing.with_transformation(Transformation::rotate(radians(
                                    angle.to_radians(),
                                )))
                            },
                        )
                })
            });

        div()
            .relative()
            .flex_none()
            .size(size)
            .child(layer("emblem/glow.svg"))
            .children(wings)
            .child(layer("emblem/halo.svg").with_animation(
                "emblem-halo",
                Animation::new(Duration::from_millis(3_600)).repeat(),
                |halo, delta| halo.opacity(0.7 + 0.3 * wave(delta)),
            ))
            .child(layer("emblem/body.svg"))
            .child(layer("emblem/pupil.svg").with_animations(
                "emblem-pupil",
                vec![
                    Animation::new(Duration::from_millis(5_400)).repeat(),
                    Animation::new(Duration::from_secs(9)).repeat(),
                ],
                move |pupil, ix, delta| match ix {
                    0 => pupil.left(unit * pupil_offset(delta)),
                    _ => pupil.opacity(blink_opacity(delta)),
                },
            ))
            .with_animation(
                "emblem-float",
                Animation::new(Duration::from_secs(3)).repeat(),
                move |emblem, delta| emblem.top(-(unit * FLOAT_AMPLITUDE * wave(delta))),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_goes_back_and_forth() {
        assert!(wave(0.).abs() < 1e-6);
        assert!((wave(0.5) - 1.).abs() < 1e-6);
        assert!(wave(1.).abs() < 1e-6);
    }

    #[test]
    fn pupil_follows_keyframes() {
        assert_eq!(pupil_offset(0.1), 0.);
        assert_eq!(pupil_offset(0.4), PUPIL_TRAVEL);
        assert_eq!(pupil_offset(0.7), -PUPIL_TRAVEL);
        assert_eq!(pupil_offset(0.95), 0.);
    }

    #[test]
    fn blink_is_brief() {
        assert_eq!(blink_opacity(0.5), 1.);
        assert_eq!(blink_opacity(0.98), 0.1);
    }
}

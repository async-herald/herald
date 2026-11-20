//! Herald Desktop : interface native (GPUI) du moteur d'analyse Herald.
//!
//! Toute l'analyse se fait sur la machine de l'utilisateur ; aucun fichier
//! source n'est envoyé sur le réseau.

// Windows, version finale : pas de fenêtre de console derrière l'app.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod assets;
mod engine;
mod export;
mod model;
mod paths;
mod service;
mod store;
mod theme;
mod time;
mod ui;

/// Version de l'app (`version` de `Cargo.toml`).
const VERSION: &str = env!("CARGO_PKG_VERSION");

use gpui::{
    App, AppContext, Application, Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions,
    WindowBounds, WindowOptions, actions, px, size,
};

use crate::ui::HeraldApp;

actions!(herald, [Quit]);

fn main() {
    Application::new()
        .with_assets(assets::Assets)
        .run(|cx: &mut App| {
            if let Err(err) = assets::register_fonts(cx) {
                eprintln!(
                    "herald: polices embarquées non chargées ({err:#}), police système utilisée"
                );
            }

            register_app_menu(cx);
            ui::bind_input_keys(cx);
            // Une seule fenêtre : la fermer quitte l'application.
            cx.on_window_closed(|cx| cx.quit()).detach();

            let bounds = Bounds::centered(None, size(px(1360.), px(860.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(980.), px(640.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Async Herald".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };

            cx.open_window(options, |window, cx| {
                cx.new(|cx| HeraldApp::new(window, cx))
            })
            .expect("impossible d'ouvrir la fenêtre principale");
            cx.activate(true);
        });
}

fn register_app_menu(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
    cx.set_menus(vec![Menu {
        name: "Async Herald".into(),
        items: vec![MenuItem::action("Quitter Async Herald", Quit)],
    }]);
}

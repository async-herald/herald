use gpui::{App, Hsla, IntoElement, Pixels, RenderOnce, Styled, Window, svg};

use crate::theme::metrics;

/// Icônes embarquées dans `assets/icons`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconName {
    ArrowRight,
    Bell,
    Check,
    ChevronDown,
    ChevronLeft,
    ChevronRight,
    Close,
    Export,
    Flag,
    FolderUp,
    GitBranch,
    GitHub,
    Lock,
    #[allow(dead_code)]
    Pencil,
    Play,
    Plus,
    Refresh,
    Sparkles,
}

impl IconName {
    fn path(self) -> &'static str {
        match self {
            Self::ArrowRight => "icons/arrow-right.svg",
            Self::Bell => "icons/bell.svg",
            Self::Check => "icons/check.svg",
            Self::ChevronDown => "icons/chevron-down.svg",
            Self::ChevronLeft => "icons/chevron-left.svg",
            Self::ChevronRight => "icons/chevron-right.svg",
            Self::Close => "icons/close.svg",
            Self::Export => "icons/export.svg",
            Self::Flag => "icons/flag.svg",
            Self::FolderUp => "icons/folder-up.svg",
            Self::GitBranch => "icons/git-branch.svg",
            Self::GitHub => "icons/github.svg",
            Self::Lock => "icons/lock.svg",
            Self::Pencil => "icons/pencil.svg",
            Self::Play => "icons/play.svg",
            Self::Plus => "icons/plus.svg",
            Self::Refresh => "icons/refresh.svg",
            Self::Sparkles => "icons/sparkles.svg",
        }
    }
}

/// Icône monochrome teintée avec `color`.
#[derive(IntoElement)]
pub struct Icon {
    name: IconName,
    size: Pixels,
    color: Hsla,
}

impl Icon {
    pub fn new(name: IconName, color: Hsla) -> Self {
        Self {
            name,
            size: metrics::ICON,
            color,
        }
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }
}

impl RenderOnce for Icon {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        svg()
            .path(self.name.path())
            .flex_none()
            .size(self.size)
            .text_color(self.color)
    }
}

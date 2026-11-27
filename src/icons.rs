//! UI のアイコン。
//!
//! アイコンは同梱の SVG で、gpui の [`AssetSource`] として読み込む。どの絵を使うかは
//! [`Icon`] で抽象化しているので、後から Nerd Font のグリフなどに差し替えられる。

use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

use crate::agents::AgentState;

/// アイコンと同梱の SVG（`assets/icons/<name>.svg`）を並べて定義する。
macro_rules! icons {
    ($($variant:ident => $name:literal,)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Icon {
            $($variant,)*
        }

        impl Icon {
            #[cfg(test)]
            pub const ALL: &[Icon] = &[$(Icon::$variant,)*];

            /// アセットのパス（`svg().path(...)` に渡す）。
            pub fn path(self) -> &'static str {
                match self {
                    $(Icon::$variant => concat!("icons/", $name, ".svg"),)*
                }
            }
        }

        /// パスと中身の組。
        const BUNDLED: &[(&str, &[u8])] = &[
            $((
                concat!("icons/", $name, ".svg"),
                include_bytes!(concat!("../assets/icons/", $name, ".svg")),
            ),)*
        ];
    };
}

icons! {
    Spinner => "spinner",
    Alert => "alert",
    Check => "check",
    Circle => "circle",
    Workspace => "workspace",
    Puppet => "puppet",
    Terminal => "terminal",
    Plus => "plus",
    ChevronLeft => "chevron-left",
    ChevronRight => "chevron-right",
    Keyboard => "keyboard",
}

/// Agent の状態を表すアイコン。色だけでなく形でも区別できるようにする。
pub fn state_icon(state: AgentState) -> Icon {
    match state {
        AgentState::Working => Icon::Spinner,
        AgentState::Blocked => Icon::Alert,
        AgentState::Done => Icon::Check,
        AgentState::Idle => Icon::Circle,
    }
}

/// 同梱のアセット。`Application::with_assets` に渡す。
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(BUNDLED
            .iter()
            .find(|(candidate, _)| *candidate == path)
            .map(|(_, data)| Cow::Borrowed(*data)))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(BUNDLED
            .iter()
            .filter(|(candidate, _)| candidate.starts_with(path))
            .map(|(candidate, _)| SharedString::from(*candidate))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_bundled_as_svg() {
        for icon in Icon::ALL {
            let data = Assets
                .load(icon.path())
                .unwrap()
                .unwrap_or_else(|| panic!("{} is bundled", icon.path()));
            let text = std::str::from_utf8(&data).unwrap();
            assert!(text.trim_start().starts_with("<svg"), "{}", icon.path());
        }
    }

    #[test]
    fn unknown_paths_are_not_found() {
        assert!(Assets.load("icons/nope.svg").unwrap().is_none());
    }

    #[test]
    fn assets_are_listed_by_prefix() {
        let listed = Assets.list("icons/").unwrap();
        assert_eq!(listed.len(), Icon::ALL.len());
        assert!(Assets.list("fonts/").unwrap().is_empty());
    }

    #[test]
    fn agent_states_have_distinct_icons() {
        assert_eq!(state_icon(AgentState::Working), Icon::Spinner);
        assert_eq!(state_icon(AgentState::Blocked), Icon::Alert);
        assert_eq!(state_icon(AgentState::Done), Icon::Check);
        assert_eq!(state_icon(AgentState::Idle), Icon::Circle);
    }
}

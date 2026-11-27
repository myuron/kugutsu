//! フォント選択（ターミナルの等幅フォントと UI のフォント）。

/// 優先順に並べた等幅フォントファミリー。
const PREFERRED_MONOSPACE: &[&str] = &[
    "JetBrains Mono",
    "JetBrainsMono Nerd Font Mono",
    "Fira Code",
    "Hack",
    "Source Code Pro",
    "DejaVu Sans Mono",
    "Noto Sans Mono",
    "Liberation Mono",
    "Ubuntu Mono",
    "Menlo",
    "Consolas",
];

/// インストール済みのフォント名から、ターミナルに使う等幅フォントを選ぶ。見つからなければ `None`。
pub fn choose_monospace_family(available: &[String]) -> Option<String> {
    let installed = |name: &str| available.iter().any(|a| a == name);
    PREFERRED_MONOSPACE
        .iter()
        .copied()
        .find(|name| installed(name))
        .or_else(|| {
            available
                .iter()
                .map(String::as_str)
                .find(|name| name.ends_with(" Mono"))
        })
        .map(str::to_string)
}

/// ターミナルに使うフォント。設定された `configured` がインストールされていればそれを、
/// 無ければ [`choose_monospace_family`] で選んだものを使う。
pub fn resolve_family(configured: Option<&str>, available: &[String]) -> Option<String> {
    if let Some(name) = configured {
        if available.iter().any(|a| a == name) {
            return Some(name.to_string());
        }
        eprintln!("warning: font_family {name:?} is not installed; using a fallback font");
    }
    choose_monospace_family(available)
}

/// UI に使う、優先順に並べたプロポーショナルフォント。
const PREFERRED_UI: &[&str] = &[
    "Inter",
    "Inter Variable",
    "IBM Plex Sans",
    "Noto Sans",
    "Cantarell",
    "Ubuntu",
    "DejaVu Sans",
    "Liberation Sans",
    "Helvetica Neue",
    "Segoe UI",
];

/// UI（バー・サイドバーなど）に使うフォント。設定された `configured` がインストールされて
/// いればそれを、無ければ優先リストから選ぶ。どれも無ければ `None`（gpui の既定）。
pub fn resolve_ui_family(configured: Option<&str>, available: &[String]) -> Option<String> {
    let installed = |name: &str| available.iter().any(|a| a == name);
    if let Some(name) = configured {
        if installed(name) {
            return Some(name.to_string());
        }
        eprintln!("warning: ui_font_family {name:?} is not installed; using a fallback font");
    }
    PREFERRED_UI
        .iter()
        .find(|name| installed(name))
        .map(|name| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn picks_most_preferred_installed_font() {
        let available = names(&["Noto Sans", "DejaVu Sans Mono", "JetBrains Mono"]);
        assert_eq!(
            choose_monospace_family(&available).as_deref(),
            Some("JetBrains Mono")
        );
    }

    #[test]
    fn skips_fonts_that_are_not_installed() {
        let available = names(&["Noto Sans", "DejaVu Sans Mono"]);
        assert_eq!(
            choose_monospace_family(&available).as_deref(),
            Some("DejaVu Sans Mono")
        );
    }

    #[test]
    fn falls_back_to_any_mono_named_font() {
        let available = names(&["Noto Sans", "Some Mono"]);
        assert_eq!(
            choose_monospace_family(&available).as_deref(),
            Some("Some Mono")
        );
    }

    #[test]
    fn does_not_pick_proportional_variants() {
        let available = names(&["JetBrainsMono Nerd Font Propo", "Noto Sans"]);
        assert_eq!(choose_monospace_family(&available), None);
    }

    #[test]
    fn resolve_uses_configured_font_when_installed() {
        let available = names(&["JetBrains Mono", "Fira Code"]);
        assert_eq!(
            resolve_family(Some("Fira Code"), &available).as_deref(),
            Some("Fira Code")
        );
    }

    #[test]
    fn resolve_falls_back_when_configured_font_is_missing() {
        let available = names(&["JetBrains Mono"]);
        assert_eq!(
            resolve_family(Some("Missing Font"), &available).as_deref(),
            Some("JetBrains Mono")
        );
    }

    #[test]
    fn resolve_chooses_monospace_without_configuration() {
        let available = names(&["Noto Sans", "Hack"]);
        assert_eq!(resolve_family(None, &available).as_deref(), Some("Hack"));
    }

    #[test]
    fn ui_font_prefers_proportional_fonts() {
        let available = names(&["DejaVu Sans Mono", "DejaVu Sans", "Inter"]);
        assert_eq!(
            resolve_ui_family(None, &available).as_deref(),
            Some("Inter")
        );
        let available = names(&["DejaVu Sans Mono", "DejaVu Sans"]);
        assert_eq!(
            resolve_ui_family(None, &available).as_deref(),
            Some("DejaVu Sans")
        );
    }

    #[test]
    fn ui_font_uses_configured_font_when_installed() {
        let available = names(&["Inter", "Fira Code"]);
        assert_eq!(
            resolve_ui_family(Some("Fira Code"), &available).as_deref(),
            Some("Fira Code")
        );
        assert_eq!(
            resolve_ui_family(Some("Missing"), &available).as_deref(),
            Some("Inter")
        );
    }

    #[test]
    fn ui_font_is_none_without_known_fonts() {
        assert_eq!(resolve_ui_family(None, &names(&["Some Mono"])), None);
    }

    #[test]
    fn falls_back_when_nothing_matches() {
        assert_eq!(choose_monospace_family(&names(&["Noto Sans"])), None);
    }
}

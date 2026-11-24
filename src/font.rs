//! ターミナル用の等幅フォント選択。

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
    fn falls_back_when_nothing_matches() {
        assert_eq!(choose_monospace_family(&names(&["Noto Sans"])), None);
    }
}

//! 右サイドバーに出すプラグインのセクション（`kugutsu.sidebar`）。
//!
//! プラグインが Lua から書き込み、ビューは描画のたびに読むだけ。描画中に Lua は呼ばない。

use std::collections::HashSet;

/// サイドバーの 1 セクション。
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub id: String,
    pub title: String,
    pub lines: Vec<String>,
}

/// セクションの一覧と、隠しているセクションの id。
#[derive(Debug, Default)]
pub struct Sidebar {
    /// 最初に `set` した順。
    sections: Vec<Section>,
    hidden: HashSet<String>,
}

impl Sidebar {
    /// セクションを追加する。同じ id があれば、位置はそのままで中身を置き換える。
    pub fn set(&mut self, section: Section) {
        match self.sections.iter_mut().find(|s| s.id == section.id) {
            Some(existing) => *existing = section,
            None => self.sections.push(section),
        }
    }

    /// セクションを消す。隠していたことも忘れる。無ければ `false`。
    pub fn del(&mut self, id: &str) -> bool {
        let Some(index) = self.sections.iter().position(|s| s.id == id) else {
            return false;
        };
        self.sections.remove(index);
        self.hidden.remove(id);
        true
    }

    /// 隠す・表示するを切り替える。無ければ `false`。
    pub fn toggle(&mut self, id: &str) -> bool {
        let hidden = self.hidden.contains(id);
        self.set_hidden(id, !hidden)
    }

    /// 表示する。無ければ `false`。
    pub fn show(&mut self, id: &str) -> bool {
        self.set_hidden(id, false)
    }

    /// 隠す。中身は残り、隠している間も `set` で更新できる。無ければ `false`。
    pub fn hide(&mut self, id: &str) -> bool {
        self.set_hidden(id, true)
    }

    fn set_hidden(&mut self, id: &str, hidden: bool) -> bool {
        if !self.sections.iter().any(|s| s.id == id) {
            return false;
        }
        if hidden {
            self.hidden.insert(id.to_string());
        } else {
            self.hidden.remove(id);
        }
        true
    }

    /// 表示するセクション（最初に `set` した順）。
    pub fn visible(&self) -> impl Iterator<Item = &Section> {
        self.sections
            .iter()
            .filter(|section| !self.hidden.contains(&section.id))
    }

    /// 隠しているセクションの id。`reload_config` で新しい Lua state に引き継ぐ。
    pub fn hidden_ids(&self) -> HashSet<String> {
        self.hidden.clone()
    }

    /// `ids` を隠す。まだ無い id も、後で `set` されたときに隠れている。
    pub fn restore_hidden(&mut self, ids: HashSet<String>) {
        self.hidden.extend(ids);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(id: &str, lines: &[&str]) -> Section {
        Section {
            id: id.into(),
            title: id.to_uppercase(),
            lines: lines.iter().map(|line| line.to_string()).collect(),
        }
    }

    fn visible(sidebar: &Sidebar) -> Vec<Section> {
        sidebar.visible().cloned().collect()
    }

    #[test]
    fn sections_are_listed_in_the_order_they_were_first_set() {
        let mut sidebar = Sidebar::default();
        assert_eq!(visible(&sidebar), []);
        sidebar.set(section("a", &["1"]));
        sidebar.set(section("b", &["2"]));
        sidebar.set(section("a", &["3"]));
        assert_eq!(
            visible(&sidebar),
            [section("a", &["3"]), section("b", &["2"])]
        );
    }

    #[test]
    fn del_removes_a_section() {
        let mut sidebar = Sidebar::default();
        sidebar.set(section("a", &[]));
        sidebar.set(section("b", &[]));
        assert!(sidebar.del("a"));
        assert!(!sidebar.del("a"));
        assert_eq!(visible(&sidebar), [section("b", &[])]);
    }

    #[test]
    fn hidden_sections_keep_receiving_updates() {
        let mut sidebar = Sidebar::default();
        sidebar.set(section("a", &["1"]));
        assert!(sidebar.toggle("a"));
        assert_eq!(visible(&sidebar), []);
        sidebar.set(section("a", &["2"]));
        assert_eq!(visible(&sidebar), []);
        assert!(sidebar.toggle("a"));
        assert_eq!(visible(&sidebar), [section("a", &["2"])]);
    }

    #[test]
    fn show_and_hide_set_visibility_explicitly() {
        let mut sidebar = Sidebar::default();
        sidebar.set(section("a", &[]));
        assert!(sidebar.hide("a"));
        assert!(sidebar.hide("a"));
        assert_eq!(visible(&sidebar), []);
        assert!(sidebar.show("a"));
        assert!(sidebar.show("a"));
        assert_eq!(visible(&sidebar), [section("a", &[])]);
    }

    #[test]
    fn unknown_ids_are_reported() {
        let mut sidebar = Sidebar::default();
        assert!(!sidebar.toggle("a"));
        assert!(!sidebar.show("a"));
        assert!(!sidebar.hide("a"));
        assert!(!sidebar.del("a"));
        assert_eq!(sidebar.hidden_ids(), HashSet::new());
    }

    #[test]
    fn del_forgets_that_a_section_was_hidden() {
        let mut sidebar = Sidebar::default();
        sidebar.set(section("a", &[]));
        sidebar.hide("a");
        sidebar.del("a");
        assert_eq!(sidebar.hidden_ids(), HashSet::new());
        sidebar.set(section("a", &[]));
        assert_eq!(visible(&sidebar), [section("a", &[])]);
    }

    #[test]
    fn restored_ids_are_hidden_when_set_again() {
        let mut old = Sidebar::default();
        old.set(section("a", &[]));
        old.set(section("b", &[]));
        old.hide("a");
        let mut new = Sidebar::default();
        new.set(section("b", &[]));
        new.restore_hidden(old.hidden_ids());
        new.set(section("a", &["new"]));
        assert_eq!(visible(&new), [section("b", &[])]);
        assert!(new.show("a"));
        assert_eq!(visible(&new), [section("b", &[]), section("a", &["new"])]);
    }
}

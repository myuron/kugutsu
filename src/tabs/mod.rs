//! 1 ウィンドウ内に並ぶタブの集合と、どれがアクティブかを管理する。

/// 順序付きのタブ集合。空になり得るが、空でない限り必ずアクティブなタブがある。
pub struct Tabs<T> {
    items: Vec<T>,
    active: usize,
}

impl<T> Tabs<T> {
    pub fn new(first: T) -> Self {
        Self {
            items: vec![first],
            active: 0,
        }
    }

    /// 末尾に追加してアクティブにする。
    pub fn push(&mut self, item: T) {
        self.items.push(item);
        self.active = self.items.len() - 1;
    }

    /// 指定位置のタブを取り除く。アクティブより前を消せばアクティブ位置を詰め、
    /// アクティブ自身を消せば右隣（末尾なら左隣）をアクティブにする。
    pub fn remove(&mut self, index: usize) -> Option<T> {
        if index >= self.items.len() {
            return None;
        }
        let removed = self.items.remove(index);
        if index < self.active || self.active == self.items.len() {
            self.active = self.active.saturating_sub(1);
        }
        Some(removed)
    }

    /// 次のタブへ（末尾からは先頭へ循環）。
    pub fn next(&mut self) {
        if !self.items.is_empty() {
            self.active = (self.active + 1) % self.items.len();
        }
    }

    /// 前のタブへ（先頭からは末尾へ循環）。
    pub fn prev(&mut self) {
        if !self.items.is_empty() {
            self.active = (self.active + self.items.len() - 1) % self.items.len();
        }
    }

    /// 指定位置をアクティブにする。範囲外なら何もしない。
    pub fn select(&mut self, index: usize) {
        if index < self.items.len() {
            self.active = index;
        }
    }

    pub fn active(&self) -> Option<&T> {
        self.items.get(self.active)
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn position(&self, predicate: impl FnMut(&T) -> bool) -> Option<usize> {
        self.items.iter().position(predicate)
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs(items: &[&'static str], active: usize) -> Tabs<&'static str> {
        let mut tabs = Tabs::new(items[0]);
        for item in &items[1..] {
            tabs.push(item);
        }
        tabs.select(active);
        tabs
    }

    #[test]
    fn starts_with_one_active_tab() {
        let tabs = Tabs::new("a");
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs.active(), Some(&"a"));
        assert_eq!(tabs.active_index(), 0);
    }

    #[test]
    fn push_appends_and_activates() {
        let mut tabs = tabs(&["a", "b"], 0);
        tabs.push("c");
        assert_eq!(tabs.iter().copied().collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(tabs.active(), Some(&"c"));
    }

    #[test]
    fn next_and_prev_wrap_around() {
        let mut tabs = tabs(&["a", "b", "c"], 2);
        tabs.next();
        assert_eq!(tabs.active(), Some(&"a"));
        tabs.prev();
        assert_eq!(tabs.active(), Some(&"c"));
        tabs.prev();
        assert_eq!(tabs.active(), Some(&"b"));
    }

    #[test]
    fn select_ignores_out_of_range() {
        let mut tabs = tabs(&["a", "b"], 1);
        tabs.select(5);
        assert_eq!(tabs.active(), Some(&"b"));
        tabs.select(0);
        assert_eq!(tabs.active(), Some(&"a"));
    }

    #[test]
    fn removing_tab_before_active_keeps_active_tab() {
        let mut tabs = tabs(&["a", "b", "c"], 2);
        assert_eq!(tabs.remove(0), Some("a"));
        assert_eq!(tabs.active(), Some(&"c"));
        assert_eq!(tabs.active_index(), 1);
    }

    #[test]
    fn removing_tab_after_active_keeps_active_tab() {
        let mut tabs = tabs(&["a", "b", "c"], 0);
        assert_eq!(tabs.remove(2), Some("c"));
        assert_eq!(tabs.active(), Some(&"a"));
    }

    #[test]
    fn removing_active_tab_activates_right_neighbor() {
        let mut tabs = tabs(&["a", "b", "c"], 1);
        tabs.remove(1);
        assert_eq!(tabs.active(), Some(&"c"));
    }

    #[test]
    fn removing_last_active_tab_activates_left_neighbor() {
        let mut tabs = tabs(&["a", "b", "c"], 2);
        tabs.remove(2);
        assert_eq!(tabs.active(), Some(&"b"));
    }

    #[test]
    fn removing_out_of_range_does_nothing() {
        let mut tabs = tabs(&["a", "b"], 1);
        assert_eq!(tabs.remove(2), None);
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs.active(), Some(&"b"));
    }

    #[test]
    fn removing_every_tab_leaves_it_empty() {
        let mut tabs = Tabs::new("a");
        tabs.remove(0);
        assert!(tabs.is_empty());
        assert_eq!(tabs.active(), None);
        // 空でも操作で panic しない
        tabs.next();
        tabs.prev();
    }

    #[test]
    fn finds_position() {
        let tabs = tabs(&["a", "b", "c"], 0);
        assert_eq!(tabs.position(|t| *t == "b"), Some(1));
        assert_eq!(tabs.position(|t| *t == "z"), None);
    }
}

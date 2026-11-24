//! 1 タブ内を分割して並べるペインの木構造と、どれがアクティブかを管理する。
//!
//! 同じ向きの分割は 1 つの節に平坦化し、兄弟同士で領域を均等に分け合う。

/// 分割の向き。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// 左右に並べる。
    Horizontal,
    /// 上下に並べる。
    Vertical,
}

/// フォーカスを移す方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// ペインの識別子。同じ [`Panes`] 内で使い回されない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaneId(usize);

/// ペイン木の節。
pub enum Node<T> {
    Leaf { id: PaneId, item: T },
    Split { axis: Axis, children: Vec<Node<T>> },
}

/// 単位正方形（0〜1）上でのペインの位置と大きさ。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// 浮動小数の誤差を吸収して境界の一致を判定するための許容幅。
const EPSILON: f32 = 1e-4;

/// ペインの木。空になり得るが、空でない限り必ずアクティブなペインがある。
pub struct Panes<T> {
    root: Option<Node<T>>,
    active: PaneId,
    next_id: usize,
}

impl<T> Panes<T> {
    pub fn new(first: T) -> Self {
        let id = PaneId(0);
        Self {
            root: Some(Node::Leaf { id, item: first }),
            active: id,
            next_id: 1,
        }
    }

    /// アクティブなペインを `axis` の向きに分割し、その直後に新しいペインを置いてアクティブにする。
    pub fn split(&mut self, axis: Axis, item: T) -> PaneId {
        let id = PaneId(self.next_id);
        self.next_id += 1;
        let leaf = Node::Leaf { id, item };
        match self.root.as_mut() {
            Some(root) => {
                if root.split(self.active, axis, leaf).is_some() {
                    unreachable!("active pane is always in the tree");
                }
            }
            None => self.root = Some(leaf),
        }
        self.active = id;
        id
    }

    /// ペインを取り除く。アクティブなペインを消した場合は、空いた領域を引き取る隣の
    /// ペイン（前の兄弟、無ければ次の兄弟）をアクティブにする。
    pub fn remove(&mut self, id: PaneId) -> Option<T> {
        let root = self.root.as_mut()?;
        let (item, successor) = if root.leaf_id() == Some(id) {
            let Some(Node::Leaf { item, .. }) = self.root.take() else {
                unreachable!()
            };
            (item, None)
        } else {
            let removed = root.remove(id)?;
            root.normalize();
            removed
        };
        if id == self.active
            && let Some(successor) = successor
        {
            self.active = successor;
        }
        Some(item)
    }

    /// 指定方向で隣接するペインへフォーカスを移す。無ければ何もしない。
    pub fn focus(&mut self, direction: Direction) {
        let layout = self.layout();
        let Some(current) = layout
            .iter()
            .find(|(id, _)| *id == self.active)
            .map(|(_, rect)| *rect)
        else {
            return;
        };
        let mut best: Option<(PaneId, f32)> = None;
        for (id, rect) in &layout {
            let Some(overlap) = neighbor_overlap(&current, rect, direction) else {
                continue;
            };
            if best.is_none_or(|(_, best_overlap)| overlap > best_overlap + EPSILON) {
                best = Some((*id, overlap));
            }
        }
        if let Some((id, _)) = best {
            self.active = id;
        }
    }

    /// 各ペインの位置（単位正方形上）を木の順に返す。
    pub fn layout(&self) -> Vec<(PaneId, Rect)> {
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            let full = Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            };
            root.layout(full, &mut out);
        }
        out
    }

    pub fn root(&self) -> Option<&Node<T>> {
        self.root.as_ref()
    }

    pub fn active(&self) -> Option<&T> {
        self.get(self.active)
    }

    pub fn active_id(&self) -> PaneId {
        self.active
    }

    /// 指定したペインをアクティブにする。存在しなければ何もしない。
    pub fn set_active(&mut self, id: PaneId) {
        if self.get(id).is_some() {
            self.active = id;
        }
    }

    pub fn get(&self, id: PaneId) -> Option<&T> {
        self.iter()
            .find(|(leaf_id, _)| *leaf_id == id)
            .map(|(_, item)| item)
    }

    pub fn find(&self, mut predicate: impl FnMut(&T) -> bool) -> Option<PaneId> {
        self.iter()
            .find(|(_, item)| predicate(item))
            .map(|(id, _)| id)
    }

    /// ペインを木の順（左上から右下）に列挙する。
    pub fn iter(&self) -> impl Iterator<Item = (PaneId, &T)> {
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            root.leaves(&mut out);
        }
        out.into_iter()
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }
}

impl<T> Node<T> {
    fn leaf_id(&self) -> Option<PaneId> {
        match self {
            Node::Leaf { id, .. } => Some(*id),
            Node::Split { .. } => None,
        }
    }

    fn first_leaf(&self) -> PaneId {
        match self {
            Node::Leaf { id, .. } => *id,
            Node::Split { children, .. } => children[0].first_leaf(),
        }
    }

    fn last_leaf(&self) -> PaneId {
        match self {
            Node::Leaf { id, .. } => *id,
            Node::Split { children, .. } => children[children.len() - 1].last_leaf(),
        }
    }

    /// `target` の葉を分割して `new` を置く。見つからなければ `new` を返す。
    fn split(&mut self, target: PaneId, axis: Axis, new: Node<T>) -> Option<Node<T>> {
        match self {
            Node::Leaf { id, .. } => {
                if *id != target {
                    return Some(new);
                }
                let placeholder = Node::Split {
                    axis,
                    children: Vec::new(),
                };
                let old = std::mem::replace(self, placeholder);
                if let Node::Split { children, .. } = self {
                    children.extend([old, new]);
                }
                None
            }
            Node::Split {
                axis: split_axis,
                children,
            } => {
                // 同じ向きの分割なら入れ子にせず兄弟として並べる
                if *split_axis == axis
                    && let Some(index) = children
                        .iter()
                        .position(|child| child.leaf_id() == Some(target))
                {
                    children.insert(index + 1, new);
                    return None;
                }
                let mut new = new;
                for child in children {
                    new = child.split(target, axis, new)?;
                }
                Some(new)
            }
        }
    }

    /// 子孫から `target` の葉を取り除き、その要素と領域を引き取るペインを返す。
    /// 呼び出し後は [`Node::normalize`] で木を整える必要がある。
    fn remove(&mut self, target: PaneId) -> Option<(T, Option<PaneId>)> {
        let Node::Split { children, .. } = self else {
            return None;
        };
        for index in 0..children.len() {
            if children[index].leaf_id() == Some(target) {
                let Node::Leaf { item, .. } = children.remove(index) else {
                    unreachable!()
                };
                let successor = match index {
                    0 => children.first().map(Node::first_leaf),
                    _ => Some(children[index - 1].last_leaf()),
                };
                return Some((item, successor));
            }
            if let Some(removed) = children[index].remove(target) {
                children[index].normalize();
                return Some(removed);
            }
        }
        None
    }

    /// 子が 1 つの分割をその子で置き換え、同じ向きの入れ子の分割を平坦化する。
    fn normalize(&mut self) {
        let Node::Split { axis, children } = self else {
            return;
        };
        let axis = *axis;
        for child in std::mem::take(children) {
            match child {
                Node::Split {
                    axis: child_axis,
                    children: grandchildren,
                } if child_axis == axis => children.extend(grandchildren),
                other => children.push(other),
            }
        }
        if children.len() == 1 {
            let only = children.pop().expect("exactly one child");
            *self = only;
        }
    }

    fn layout(&self, rect: Rect, out: &mut Vec<(PaneId, Rect)>) {
        match self {
            Node::Leaf { id, .. } => out.push((*id, rect)),
            Node::Split { axis, children } => {
                let count = children.len() as f32;
                for (index, child) in children.iter().enumerate() {
                    let offset = index as f32 / count;
                    let child_rect = match axis {
                        Axis::Horizontal => Rect {
                            x: rect.x + rect.width * offset,
                            width: rect.width / count,
                            ..rect
                        },
                        Axis::Vertical => Rect {
                            y: rect.y + rect.height * offset,
                            height: rect.height / count,
                            ..rect
                        },
                    };
                    child.layout(child_rect, out);
                }
            }
        }
    }

    fn leaves<'a>(&'a self, out: &mut Vec<(PaneId, &'a T)>) {
        match self {
            Node::Leaf { id, item } => out.push((*id, item)),
            Node::Split { children, .. } => {
                for child in children {
                    child.leaves(out);
                }
            }
        }
    }
}

/// `candidate` が `current` の `direction` 側に接していれば、接している辺の重なりの長さを返す。
fn neighbor_overlap(current: &Rect, candidate: &Rect, direction: Direction) -> Option<f32> {
    let touches = match direction {
        Direction::Left => candidate.x + candidate.width - current.x,
        Direction::Right => current.x + current.width - candidate.x,
        Direction::Up => candidate.y + candidate.height - current.y,
        Direction::Down => current.y + current.height - candidate.y,
    }
    .abs()
        < EPSILON;
    let overlap = match direction {
        Direction::Left | Direction::Right => {
            (current.y + current.height).min(candidate.y + candidate.height)
                - current.y.max(candidate.y)
        }
        Direction::Up | Direction::Down => {
            (current.x + current.width).min(candidate.x + candidate.width)
                - current.x.max(candidate.x)
        }
    };
    (touches && overlap > EPSILON).then_some(overlap)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(panes: &Panes<&'static str>) -> Vec<&'static str> {
        panes.iter().map(|(_, item)| *item).collect()
    }

    fn id_of(panes: &Panes<&'static str>, name: &str) -> PaneId {
        panes.find(|item| *item == name).unwrap()
    }

    fn rect_of(panes: &Panes<&'static str>, name: &str) -> Rect {
        let id = id_of(panes, name);
        panes
            .layout()
            .into_iter()
            .find(|(leaf, _)| *leaf == id)
            .unwrap()
            .1
    }

    fn assert_rect(actual: Rect, expected: (f32, f32, f32, f32)) {
        let (x, y, width, height) = expected;
        let close = |a: f32, b: f32| (a - b).abs() < EPSILON;
        assert!(
            close(actual.x, x)
                && close(actual.y, y)
                && close(actual.width, width)
                && close(actual.height, height),
            "{actual:?} != {expected:?}"
        );
    }

    /// 左に a、右に上下分割された b / c がある配置。
    fn l_shape() -> Panes<&'static str> {
        let mut panes = Panes::new("a");
        panes.split(Axis::Horizontal, "b");
        panes.split(Axis::Vertical, "c");
        panes
    }

    #[test]
    fn starts_with_one_active_pane() {
        let panes = Panes::new("a");
        assert_eq!(items(&panes), ["a"]);
        assert_eq!(panes.active(), Some(&"a"));
        assert_rect(rect_of(&panes, "a"), (0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn split_places_new_pane_after_active_and_activates_it() {
        let mut panes = Panes::new("a");
        let id = panes.split(Axis::Horizontal, "b");
        assert_eq!(items(&panes), ["a", "b"]);
        assert_eq!(panes.active_id(), id);
        assert_eq!(panes.active(), Some(&"b"));
        assert_rect(rect_of(&panes, "a"), (0.0, 0.0, 0.5, 1.0));
        assert_rect(rect_of(&panes, "b"), (0.5, 0.0, 0.5, 1.0));
    }

    #[test]
    fn splitting_in_same_direction_shares_space_equally() {
        let mut panes = Panes::new("a");
        panes.split(Axis::Horizontal, "c");
        panes.set_active(id_of(&panes, "a"));
        panes.split(Axis::Horizontal, "b");
        assert_eq!(items(&panes), ["a", "b", "c"]);
        let third = 1.0 / 3.0;
        assert_rect(rect_of(&panes, "a"), (0.0, 0.0, third, 1.0));
        assert_rect(rect_of(&panes, "b"), (third, 0.0, third, 1.0));
        assert_rect(rect_of(&panes, "c"), (2.0 * third, 0.0, third, 1.0));
    }

    #[test]
    fn splitting_in_other_direction_nests() {
        let panes = l_shape();
        assert_eq!(items(&panes), ["a", "b", "c"]);
        assert_rect(rect_of(&panes, "a"), (0.0, 0.0, 0.5, 1.0));
        assert_rect(rect_of(&panes, "b"), (0.5, 0.0, 0.5, 0.5));
        assert_rect(rect_of(&panes, "c"), (0.5, 0.5, 0.5, 0.5));
    }

    #[test]
    fn removing_active_pane_activates_previous_sibling() {
        let mut panes = l_shape();
        assert_eq!(panes.remove(id_of(&panes, "c")), Some("c"));
        assert_eq!(panes.active(), Some(&"b"));
        assert_rect(rect_of(&panes, "b"), (0.5, 0.0, 0.5, 1.0));
    }

    #[test]
    fn removing_first_active_pane_activates_next_sibling() {
        let mut panes = l_shape();
        panes.set_active(id_of(&panes, "a"));
        panes.remove(id_of(&panes, "a"));
        // 次の兄弟は b / c の分割なので、その先頭の b を選ぶ
        assert_eq!(panes.active(), Some(&"b"));
        assert_rect(rect_of(&panes, "b"), (0.0, 0.0, 1.0, 0.5));
        assert_rect(rect_of(&panes, "c"), (0.0, 0.5, 1.0, 0.5));
    }

    #[test]
    fn removing_previous_sibling_subtree_picks_its_last_leaf() {
        // 左に上下分割された a / b、右に c
        let mut panes = Panes::new("a");
        panes.split(Axis::Horizontal, "c");
        panes.set_active(id_of(&panes, "a"));
        panes.split(Axis::Vertical, "b");
        panes.set_active(id_of(&panes, "c"));
        panes.remove(id_of(&panes, "c"));
        assert_eq!(panes.active(), Some(&"b"));
    }

    #[test]
    fn removing_inactive_pane_keeps_active() {
        let mut panes = l_shape();
        panes.remove(id_of(&panes, "a"));
        assert_eq!(panes.active(), Some(&"c"));
    }

    #[test]
    fn collapsing_flattens_same_direction_splits() {
        // a | (b / c) から c を消すと a | b になり、そこで左右分割すると 3 等分になる
        let mut panes = l_shape();
        panes.remove(id_of(&panes, "c"));
        panes.split(Axis::Horizontal, "d");
        assert_eq!(items(&panes), ["a", "b", "d"]);
        assert_rect(rect_of(&panes, "d"), (2.0 / 3.0, 0.0, 1.0 / 3.0, 1.0));
    }

    #[test]
    fn removing_unknown_pane_does_nothing() {
        let mut panes = l_shape();
        let a = id_of(&panes, "a");
        panes.remove(a);
        assert_eq!(panes.remove(a), None);
        assert_eq!(items(&panes), ["b", "c"]);
    }

    #[test]
    fn removing_every_pane_leaves_it_empty() {
        let mut panes = Panes::new("a");
        panes.split(Axis::Vertical, "b");
        panes.remove(id_of(&panes, "a"));
        assert_eq!(panes.active(), Some(&"b"));
        panes.remove(id_of(&panes, "b"));
        assert!(panes.is_empty());
        assert_eq!(panes.active(), None);
        assert!(panes.layout().is_empty());
        // 空でも操作で panic しない
        panes.focus(Direction::Left);
    }

    #[test]
    fn set_active_ignores_unknown_pane() {
        let mut panes = l_shape();
        let a = id_of(&panes, "a");
        panes.remove(a);
        panes.set_active(a);
        assert_eq!(panes.active(), Some(&"c"));
    }

    #[test]
    fn focus_moves_to_adjacent_pane() {
        let mut panes = l_shape();
        panes.focus(Direction::Up);
        assert_eq!(panes.active(), Some(&"b"));
        panes.focus(Direction::Down);
        assert_eq!(panes.active(), Some(&"c"));
        panes.focus(Direction::Left);
        assert_eq!(panes.active(), Some(&"a"));
        panes.focus(Direction::Right);
        assert_eq!(panes.active(), Some(&"b"));
    }

    #[test]
    fn focus_stays_at_edge() {
        let mut panes = l_shape();
        for direction in [Direction::Right, Direction::Down] {
            panes.focus(direction);
            assert_eq!(panes.active(), Some(&"c"), "{direction:?}");
        }
        panes.set_active(id_of(&panes, "a"));
        for direction in [Direction::Left, Direction::Up, Direction::Down] {
            panes.focus(direction);
            assert_eq!(panes.active(), Some(&"a"), "{direction:?}");
        }
    }

    #[test]
    fn focus_prefers_largest_overlap() {
        // 上段に a | b、下段に c。c から上へは重なりが同じなので先頭の a を選ぶ
        let mut panes = Panes::new("a");
        panes.split(Axis::Vertical, "c");
        panes.set_active(id_of(&panes, "a"));
        panes.split(Axis::Horizontal, "b");
        panes.set_active(id_of(&panes, "c"));
        panes.focus(Direction::Up);
        assert_eq!(panes.active(), Some(&"a"));

        // 下段を c | d にすると、d の上には b だけが接する
        panes.set_active(id_of(&panes, "c"));
        panes.split(Axis::Horizontal, "d");
        panes.focus(Direction::Up);
        assert_eq!(panes.active(), Some(&"b"));
    }
}

//! N-ary equal-split pane layout (`PaneNode`, `SplitAxis`, `WorkspaceTab`)
//! inspired by `tty7`, supporting equal horizontal/vertical terminal splits,
//! pane focus navigation, pane zoom, and Terminal vs. Agent Chat view switching.

use gpui::{
    AnyElement, App, Entity, Focusable as _, IntoElement, ParentElement as _, SharedString,
    Styled as _, Window, div, px,
};
use gpui_component::{
    ActiveTheme as _,
    resizable::{h_resizable, resizable_panel, v_resizable},
};

use crate::terminal::view::TerminalView;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabViewMode {
    #[default]
    Terminal,
    Chat,
}

/// N-ary split pane tree: splitting along the same axis inserts a sibling into `children`
/// so that `N` splits along the same axis always divide the space into `N` equal `1/N` panes.
#[derive(Clone, Debug, PartialEq)]
pub enum PaneTree<L> {
    Leaf(L),
    Split {
        axis: SplitAxis,
        children: Vec<PaneTree<L>>,
    },
}

impl<L: Clone> PaneTree<L> {
    pub fn new_leaf(leaf: L) -> Self {
        Self::Leaf(leaf)
    }

    pub fn first_leaf(&self) -> L {
        match self {
            Self::Leaf(leaf) => leaf.clone(),
            Self::Split { children, .. } => children
                .first()
                .expect("PaneTree::Split always has at least 2 children")
                .first_leaf(),
        }
    }

    pub fn leaves(&self) -> Vec<L> {
        let mut out = Vec::new();
        self.collect_leaves(&mut out);
        out
    }

    fn collect_leaves(&self, out: &mut Vec<L>) {
        match self {
            Self::Leaf(leaf) => out.push(leaf.clone()),
            Self::Split { children, .. } => {
                for child in children {
                    child.collect_leaves(out);
                }
            }
        }
    }

    pub fn contains_where(&self, pred: &impl Fn(&L) -> bool) -> bool {
        match self {
            Self::Leaf(leaf) => pred(leaf),
            Self::Split { children, .. } => children.iter().any(|c| c.contains_where(pred)),
        }
    }

    pub fn find_where(&self, pred: &impl Fn(&L) -> bool) -> Option<L> {
        match self {
            Self::Leaf(leaf) => pred(leaf).then(|| leaf.clone()),
            Self::Split { children, .. } => children.iter().find_map(|c| c.find_where(pred)),
        }
    }

    /// Splits the leaf matching `is_target` along `axis`.
    /// When the target leaf is already a direct child of a `Split` with the same `axis`,
    /// the new leaf is inserted right after the target in `children` so all siblings share
    /// equal `1/N` width/height.
    pub fn split_where(
        &mut self,
        is_target: &impl Fn(&L) -> bool,
        new_leaf: L,
        axis: SplitAxis,
    ) -> bool {
        match self {
            Self::Leaf(leaf) => {
                if is_target(leaf) {
                    let old = Self::Leaf(leaf.clone());
                    let added = Self::Leaf(new_leaf);
                    *self = Self::Split {
                        axis,
                        children: vec![old, added],
                    };
                    true
                } else {
                    false
                }
            }
            Self::Split {
                axis: cur_axis,
                children,
            } => {
                if *cur_axis == axis {
                    if let Some(pos) = children
                        .iter()
                        .position(|c| matches!(c, Self::Leaf(l) if is_target(l)))
                    {
                        children.insert(pos + 1, Self::Leaf(new_leaf));
                        return true;
                    }
                }
                for child in children.iter_mut() {
                    if child.split_where(is_target, new_leaf.clone(), axis) {
                        return true;
                    }
                }
                false
            }
        }
    }

    /// Removes the leaf matching `is_target`.
    /// Returns `Ok(true)` if removed and the tree still has nodes,
    /// `Err(())` if `self` was the single root leaf matching `is_target`,
    /// or `Ok(false)` if not found.
    pub fn remove_where(&mut self, is_target: &impl Fn(&L) -> bool) -> Result<bool, ()> {
        match self {
            Self::Leaf(leaf) => {
                if is_target(leaf) {
                    Err(())
                } else {
                    Ok(false)
                }
            }
            Self::Split { children, .. } => {
                for idx in 0..children.len() {
                    match children[idx].remove_where(is_target) {
                        Err(()) => {
                            children.remove(idx);
                            if children.len() == 1 {
                                *self = children.remove(0);
                            }
                            return Ok(true);
                        }
                        Ok(true) => return Ok(true),
                        Ok(false) => {}
                    }
                }
                Ok(false)
            }
        }
    }
}

pub type PaneNode = PaneTree<Entity<TerminalView>>;

impl PaneNode {
    pub fn new_terminal(view: Entity<TerminalView>) -> Self {
        Self::new_leaf(view)
    }

    pub fn first_terminal(&self) -> Entity<TerminalView> {
        self.first_leaf()
    }

    pub fn terminals(&self) -> Vec<Entity<TerminalView>> {
        self.leaves()
    }

    pub fn contains_terminal(&self, target: &Entity<TerminalView>) -> bool {
        let id = target.entity_id();
        self.contains_where(&|v| v.entity_id() == id)
    }

    pub fn focused_terminal(&self, window: &Window, cx: &App) -> Option<Entity<TerminalView>> {
        self.find_where(&|v| v.read(cx).focus_handle(cx).is_focused(window))
    }

    pub fn split_target(
        &mut self,
        target: &Entity<TerminalView>,
        new_view: Entity<TerminalView>,
        axis: SplitAxis,
    ) -> bool {
        let id = target.entity_id();
        self.split_where(&|v| v.entity_id() == id, new_view, axis)
    }

    pub fn remove_terminal(&mut self, target: &Entity<TerminalView>) -> Result<bool, ()> {
        let id = target.entity_id();
        self.remove_where(&|v| v.entity_id() == id)
    }

    pub fn render(&self, focused_id: Option<gpui::EntityId>, cx: &App) -> AnyElement {
        let has_splits = matches!(self, Self::Split { .. });
        self.render_inner("root", focused_id, has_splits, cx)
    }

    fn render_inner(
        &self,
        path: &str,
        focused_id: Option<gpui::EntityId>,
        has_splits: bool,
        cx: &App,
    ) -> AnyElement {
        let border_color = cx.theme().border;
        let active_border = cx.theme().primary;

        match self {
            Self::Leaf(view) => {
                if !has_splits {
                    // Single pane: zero chrome overhead, full terminal viewport just like tty7
                    return div()
                        .size_full()
                        .overflow_hidden()
                        .child(view.clone())
                        .into_any_element();
                }

                let is_focused = focused_id == Some(view.entity_id());
                let session_name: SharedString = view.read(cx).session_name.clone().into();
                let (cols, rows) = view.read(cx).dimensions();
                let badge_text: SharedString = format!("{session_name} · {cols}×{rows}").into();

                div()
                    .size_full()
                    .relative()
                    .overflow_hidden()
                    .border_1()
                    .border_color(if is_focused {
                        active_border.opacity(0.65)
                    } else {
                        border_color.opacity(0.2)
                    })
                    .child(view.clone())
                    .child(
                        div()
                            .absolute()
                            .top_1()
                            .right_3()
                            .px_1p5()
                            .py_0p5()
                            .rounded(px(4.0))
                            .bg(cx.theme().tab_bar.opacity(0.82))
                            .border_1()
                            .border_color(if is_focused {
                                active_border.opacity(0.45)
                            } else {
                                border_color.opacity(0.35)
                            })
                            .text_xs()
                            .text_color(if is_focused {
                                cx.theme().foreground
                            } else {
                                cx.theme().muted_foreground
                            })
                            .child(badge_text),
                    )
                    .into_any_element()
            }
            Self::Split { axis, children } => {
                // Encoding `children.len()` into `group_id` guarantees that adding or removing
                // a split resets the sibling panes to equal `1/N` widths/heights, while keeping
                // interactive drag-resized ratios stable across normal renders and window resizes.
                // Double-clicking any divider handle also equalizes all panes in the group.
                let group_id = SharedString::from(format!(
                    "pane-split-{path}-{axis:?}-n{}",
                    children.len()
                ));
                let mut group = match axis {
                    SplitAxis::Horizontal => h_resizable(group_id),
                    SplitAxis::Vertical => v_resizable(group_id),
                };
                for (idx, child) in children.iter().enumerate() {
                    let child_path = format!("{path}-{idx}");
                    group = group.child(
                        resizable_panel()
                            .size_range(px(64.0)..gpui::Pixels::MAX)
                            .child(child.render_inner(&child_path, focused_id, true, cx)),
                    );
                }
                group.into_any_element()
            }
        }
    }
}

pub struct WorkspaceTab {
    pub id: u64,
    pub root: PaneNode,
    pub active_terminal: Entity<TerminalView>,
    pub zoomed_terminal: Option<Entity<TerminalView>>,
    pub view_mode: TabViewMode,
}

impl WorkspaceTab {
    pub fn new(id: u64, view: Entity<TerminalView>) -> Self {
        Self {
            id,
            root: PaneNode::new_terminal(view.clone()),
            active_terminal: view,
            zoomed_terminal: None,
            view_mode: TabViewMode::Terminal,
        }
    }

    pub fn sync_active_from_focus(&mut self, window: &Window, cx: &App) {
        if let Some(focused) = self.root.focused_terminal(window, cx) {
            self.active_terminal = focused;
        } else if !self.root.contains_terminal(&self.active_terminal) {
            self.active_terminal = self.root.first_terminal();
        }
    }

    pub fn focus_active(&self, window: &mut Window, cx: &mut App) {
        let fh = self.active_terminal.read(cx).focus_handle(cx);
        window.focus(&fh, cx);
    }

    pub fn focus_next_pane(&mut self, window: &mut Window, cx: &mut App) {
        let terms = self.root.terminals();
        if terms.len() <= 1 {
            return;
        }
        let current_idx = terms
            .iter()
            .position(|v| v.entity_id() == self.active_terminal.entity_id())
            .unwrap_or(0);
        let next = terms[(current_idx + 1) % terms.len()].clone();
        self.active_terminal = next.clone();
        if self.zoomed_terminal.is_some() {
            self.zoomed_terminal = Some(next.clone());
        }
        let fh = next.read(cx).focus_handle(cx);
        window.focus(&fh, cx);
    }

    pub fn toggle_zoom(&mut self) {
        if self.zoomed_terminal.is_some() {
            self.zoomed_terminal = None;
        } else {
            self.zoomed_terminal = Some(self.active_terminal.clone());
        }
    }

    pub fn title(&self, cx: &App) -> String {
        let term = self.active_terminal.read(cx);
        let count = self.root.terminals().len();
        if count > 1 {
            format!("{} ({count})", term.session_name)
        } else {
            term.session_name.clone()
        }
    }

    pub fn active_session_key(&self, cx: &App) -> (String, u64) {
        let term = self.active_terminal.read(cx);
        (term.server_id.clone(), term.session_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_axis_equality() {
        assert_ne!(SplitAxis::Horizontal, SplitAxis::Vertical);
        assert_eq!(TabViewMode::default(), TabViewMode::Terminal);
    }

    #[test]
    fn multiple_splits_along_same_axis_produce_flat_equal_children() {
        let mut tree = PaneTree::new_leaf(1u32);
        // First vertical split: [1, 2]
        assert!(tree.split_where(&|&x| x == 1, 2, SplitAxis::Vertical));
        // Second vertical split on pane 2: [1, 2, 3] (3 equal 1/3 siblings, NOT 1/2 + 1/4 + 1/4)
        assert!(tree.split_where(&|&x| x == 2, 3, SplitAxis::Vertical));
        assert_eq!(
            tree,
            PaneTree::Split {
                axis: SplitAxis::Vertical,
                children: vec![
                    PaneTree::Leaf(1),
                    PaneTree::Leaf(2),
                    PaneTree::Leaf(3),
                ],
            }
        );

        // Removing middle child keeps [1, 3]
        assert_eq!(tree.remove_where(&|&x| x == 2), Ok(true));
        assert_eq!(tree.leaves(), vec![1, 3]);

        // Removing another child collapses back to Leaf(3)
        assert_eq!(tree.remove_where(&|&x| x == 1), Ok(true));
        assert_eq!(tree, PaneTree::Leaf(3));
    }
}

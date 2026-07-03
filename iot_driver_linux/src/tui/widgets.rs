//! Reusable TUI widgets shared across tabs.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
    Frame,
};

/// A modal, centered single-choice list selector over arbitrary values.
///
/// Generic over the payload `T` each row carries, so it can drive any
/// enum-like choice — key trigger mode, LED mode, a key to bind, an audio
/// style, etc. Navigation is decoupled from rendering so the selection logic
/// is unit-testable without a [`Frame`].
#[derive(Debug, Clone)]
pub(crate) struct PopupSelect<T> {
    title: String,
    items: Vec<(String, T)>,
    state: ListState,
}

impl<T> PopupSelect<T> {
    /// Build a selector from `(label, value)` rows. The first row is
    /// preselected.
    pub(crate) fn new(title: impl Into<String>, items: Vec<(String, T)>) -> Self {
        let mut state = ListState::default();
        if !items.is_empty() {
            state.select(Some(0));
        }
        Self {
            title: title.into(),
            items,
            state,
        }
    }

    /// Move the selection up one row (saturating at the top).
    pub(crate) fn up(&mut self) {
        let i = self.state.selected().unwrap_or(0);
        self.state.select(Some(i.saturating_sub(1)));
    }

    /// Move the selection down one row (saturating at the bottom).
    pub(crate) fn down(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let i = self.state.selected().unwrap_or(0);
        self.state.select(Some((i + 1).min(self.items.len() - 1)));
    }

    /// The value of the currently highlighted row, if any.
    pub(crate) fn selected(&self) -> Option<&T> {
        self.items.get(self.state.selected()?).map(|(_, v)| v)
    }

    /// Preselect the first row whose value satisfies `pred` (no-op if none
    /// match), so the popup opens on the current value.
    pub(crate) fn select_where(&mut self, pred: impl Fn(&T) -> bool) {
        if let Some(i) = self.items.iter().position(|(_, v)| pred(v)) {
            self.state.select(Some(i));
        }
    }

    /// Render centered over `area`, sized to content and clamped to `area`.
    pub(crate) fn render(&mut self, f: &mut Frame, area: Rect) {
        let content_w = self
            .items
            .iter()
            .map(|(l, _)| l.len())
            .max()
            .unwrap_or(0)
            .max(self.title.len()) as u16;
        // +2 borders, +2 for the "> " highlight symbol.
        let width = (content_w + 4).min(area.width);
        let height = (self.items.len() as u16 + 2).min(area.height);
        let x = area.x + area.width.saturating_sub(width) / 2;
        let y = area.y + area.height.saturating_sub(height) / 2;
        let popup = Rect::new(x, y, width, height);

        let items: Vec<ListItem> = self
            .items
            .iter()
            .map(|(label, _)| ListItem::new(label.as_str()))
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(self.title.clone()),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        f.render_widget(Clear, popup);
        f.render_stateful_widget(list, popup, &mut self.state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PopupSelect<u8> {
        PopupSelect::new("t", vec![("a".into(), 1), ("b".into(), 2), ("c".into(), 3)])
    }

    #[test]
    fn navigation_saturates_at_both_ends() {
        let mut p = sample();
        assert_eq!(p.selected(), Some(&1));
        p.up();
        assert_eq!(p.selected(), Some(&1)); // saturates at top
        p.down();
        p.down();
        assert_eq!(p.selected(), Some(&3));
        p.down();
        assert_eq!(p.selected(), Some(&3)); // saturates at bottom
    }

    #[test]
    fn select_where_preselects_matching_row() {
        let mut p = sample();
        p.select_where(|&v| v == 2);
        assert_eq!(p.selected(), Some(&2));
        p.select_where(|&v| v == 99); // no match → unchanged
        assert_eq!(p.selected(), Some(&2));
    }

    #[test]
    fn empty_selector_has_no_selection() {
        let p: PopupSelect<u8> = PopupSelect::new("t", vec![]);
        assert_eq!(p.selected(), None);
    }
}

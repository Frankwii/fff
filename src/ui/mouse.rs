//! The mouse: clicks and the wheel in the results and lists, drags that
//! mark rows and scroll past an edge, and the opt-in button bar.
use super::*;

/// Two clicks on the same row within this time act like `l`.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// Rows the wheel scrolls per step.
const WHEEL_ROWS: usize = 3;
/// While a drag is held past the results' top or bottom edge, they scroll a
/// row this often, marking as they go.
pub(super) const EDGE_STEP: Duration = Duration::from_millis(50);
/// The mouse in the results, like a graphical file manager.
#[derive(Default)]
pub(super) struct Mouse {
    /// The last click, for double clicks.
    click: Option<(PathBuf, std::time::Instant)>,
    /// Where a held left button went down; dragging marks rows from here.
    anchor: Option<PathBuf>,
    /// Where `anchor` was last found in the rows.
    anchor_at: usize,
    /// A drag held past the top or bottom edge, and when it next scrolls.
    pub(super) edge: Option<(MouseEvent, std::time::Instant)>,
    /// The last click on a mark or a chooser row, for double clicks.
    item: Option<(usize, std::time::Instant)>,
}
impl Mouse {
    /// A click on list item `index`: true when it is the second on it.
    fn again(&mut self, index: usize) -> bool {
        let now = std::time::Instant::now();
        let again = self
            .item
            .take()
            .is_some_and(|(i, at)| i == index && now.duration_since(at) < DOUBLE_CLICK);
        if !again {
            self.item = Some((index, now));
        }
        again
    }
    /// When a drag held past an edge scrolls next.
    pub(super) fn edge_due(&self) -> Option<std::time::Instant> {
        self.edge.map(|(_, at)| at)
    }
}
/// `[ui] mouse` in the results. A click clears the marks and highlights a
/// row; a second click on it acts like `l`, or in a dialog (fff explorer)
/// returns true for the caller to deliver Enter on it; dragging with the
/// button held marks every row from the pressed one to the current one, and
/// past the top or bottom edge scrolls on (see `drag_at_edge`). The wheel
/// only scrolls. `area` holds the rows of the last frame, one per rendered
/// target.
pub(super) fn mouse_results(
    app: &mut App,
    discovery: &mut Discovery,
    event: MouseEvent,
    area: ratatui::layout::Rect,
    mouse: &mut Mouse,
) -> bool {
    let inside = event.column >= area.x && event.column < area.x + area.width;
    let row = (inside && event.row >= area.y)
        .then(|| app.rendered.get(usize::from(event.row - area.y)).cloned())
        .flatten();
    match event.kind {
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
            let last = app
                .rows
                .len()
                .saturating_sub(usize::from(area.height).max(1));
            app.offset = if event.kind == MouseEventKind::ScrollDown {
                (app.offset + WHEEL_ROWS).min(last)
            } else {
                app.offset.saturating_sub(WHEEL_ROWS)
            };
            app.scrolled = Some(app.highlight.clone());
        }
        MouseEventKind::Down(MouseButton::Left) => {
            mouse.edge = None;
            let Some(path) = row else {
                return false;
            };
            app.marks.clear();
            let now = std::time::Instant::now();
            let double = mouse
                .click
                .take()
                .is_some_and(|(p, at)| p == path && now.duration_since(at) < DOUBLE_CLICK);
            app.highlight = Some(path.clone());
            app.highlight_top = false;
            app.wanted = None;
            if double {
                mouse.anchor = None;
                if app.dialog.is_some() {
                    return true;
                }
                discovery.checkpoint(app);
                activate_highlighted(app, discovery);
            } else {
                mouse.click = Some((path.clone(), now));
                mouse.anchor = Some(path);
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            let past = event.row < area.y || event.row >= area.y + area.height;
            mouse.edge = (past && mouse.anchor.is_some())
                .then(|| (event, std::time::Instant::now() + EDGE_STEP));
            drag(app, event, area, mouse);
        }
        MouseEventKind::Up(MouseButton::Left) => {
            mouse.anchor = None;
            mouse.edge = None;
        }
        _ => {}
    }
    false
}
/// What the mouse did in the marked list or a chooser.
#[derive(Debug, PartialEq)]
pub(super) enum ListMouse {
    /// Neither was under it: the results take it.
    Pass,
    /// Handled (or ignored) there.
    Done,
    /// A second click on a chooser row: Enter chooses it.
    Enter,
    /// A second click on a mark: `l` on it, or Enter in a dialog.
    Open(PathBuf),
}
/// Where the last frame drew the marked list and the open chooser's rows
/// (see `view::Rendered`).
#[derive(Default)]
pub(super) struct ListAreas {
    pub marks_pane: ratatui::layout::Rect,
    pub marks: Vec<(ratatui::layout::Rect, usize)>,
    pub choices: Vec<(ratatui::layout::Rect, usize)>,
}
/// The row a wheel step moves to from `current` among `count`.
pub(super) fn wheel_step(current: Option<usize>, count: usize, forward: bool) -> Option<usize> {
    let last = count.checked_sub(1)?;
    match current {
        None => forward.then_some(0),
        Some(i) if forward => Some((i + 1).min(last)),
        Some(i) => Some(i.saturating_sub(1).min(last)),
    }
}
/// `[ui] mouse` in the marked list and the choosers (bookmarks, their icons,
/// palettes, go-to suggestions). A click on a row selects it (a mark focuses
/// the marked list) and a second click on it acts on it; the wheel over
/// them moves the selection. A click outside the marked list returns to the
/// results. While a chooser is open, the mouse does nothing else.
pub(super) fn mouse_lists(
    app: &App,
    focus: &mut Focus,
    event: MouseEvent,
    areas: &ListAreas,
    mouse: &mut Mouse,
) -> ListMouse {
    let position = ratatui::layout::Position::new(event.column, event.row);
    let hit = |list: &[(ratatui::layout::Rect, usize)]| {
        list.iter()
            .find(|(area, _)| area.contains(position))
            .map(|&(_, i)| i)
    };
    let down = event.kind == MouseEventKind::Down(MouseButton::Left);
    let wheel = match event.kind {
        MouseEventKind::ScrollDown => Some(true),
        MouseEventKind::ScrollUp => Some(false),
        _ => None,
    };
    let chooser = match &*focus {
        Focus::Bookmarks { query, index } => {
            Some((Some(*index), app.matching_bookmarks(query).len()))
        }
        Focus::BookmarkIcons { query, index, .. } => Some((
            Some(*index),
            crate::icons::matching_bookmark_icons(query).len(),
        )),
        Focus::Palettes { query, index, .. } => {
            Some((Some(*index), app.matching_palettes(query).len()))
        }
        Focus::Location(location) => Some((location.selected, location.matches().len())),
        _ => None,
    };
    if let Some((current, count)) = chooser {
        let target = match wheel {
            Some(forward) => wheel_step(current, count, forward),
            None if down => hit(&areas.choices),
            None => None,
        };
        let Some(target) = target else {
            return ListMouse::Done;
        };
        let again = down && mouse.again(target);
        match focus {
            Focus::Bookmarks { index, .. }
            | Focus::BookmarkIcons { index, .. }
            | Focus::Palettes { index, .. } => *index = target,
            Focus::Location(location) => location.selected = Some(target),
            _ => {}
        }
        return if again {
            ListMouse::Enter
        } else {
            ListMouse::Done
        };
    }
    if !matches!(focus, Focus::Results | Focus::Marks { .. }) {
        return ListMouse::Pass;
    }
    let over_marks = areas.marks_pane.contains(position);
    let select = |focus: &mut Focus, target: usize| match focus {
        Focus::Marks { index, .. } => *index = target,
        _ => {
            *focus = Focus::Marks {
                rows: app.marks.to_vec(),
                index: target,
            }
        }
    };
    if down {
        if let Some(target) = hit(&areas.marks) {
            let again = mouse.again(target);
            select(focus, target);
            if again && let Focus::Marks { rows, index } = focus {
                return rows
                    .get(*index)
                    .cloned()
                    .map_or(ListMouse::Done, ListMouse::Open);
            }
            return ListMouse::Done;
        }
        if over_marks {
            return ListMouse::Done;
        }
        mouse.item = None;
        if matches!(focus, Focus::Marks { .. }) {
            *focus = Focus::Results;
        }
        return ListMouse::Pass;
    }
    if let Some(forward) = wheel
        && over_marks
    {
        let (count, current) = match focus {
            Focus::Marks { rows, index } => (rows.len(), Some(*index)),
            _ => (app.marks.len(), None),
        };
        if let Some(target) = wheel_step(current, count, forward) {
            select(focus, target);
        }
        return ListMouse::Done;
    }
    if matches!(focus, Focus::Marks { .. }) {
        return ListMouse::Done;
    }
    ListMouse::Pass
}
/// A drag held past the results' edge without moving: scroll a row further
/// and mark it, as the drag would. True when it did.
pub(super) fn drag_at_edge(
    app: &mut App,
    area: ratatui::layout::Rect,
    mouse: &mut Mouse,
    now: std::time::Instant,
) -> bool {
    let Some((event, at)) = mouse.edge else {
        return false;
    };
    if now < at {
        return false;
    }
    mouse.edge = Some((event, now + EDGE_STEP));
    drag(app, event, area, mouse)
}
/// Mark the rows from the anchor to the one under `event`; past the top or
/// bottom edge, to the row beyond the visible ones, which the highlight then
/// scrolls into view. True when the marks changed.
fn drag(app: &mut App, event: MouseEvent, area: ratatui::layout::Rect, mouse: &mut Mouse) -> bool {
    let Some(anchor) = mouse.anchor.as_ref() else {
        return false;
    };
    let Some(last) = app.rows.len().checked_sub(1) else {
        return false;
    };
    // The anchor is usually still on screen or where it was last found.
    let from = app
        .rendered
        .iter()
        .position(|p| p == anchor)
        .map(|i| app.offset + i)
        .filter(|&i| app.rows.get(i).is_some_and(|c| c.path == *anchor))
        .or_else(|| {
            let hint = mouse.anchor_at;
            app.rows
                .get(hint)
                .is_some_and(|c| c.path == *anchor)
                .then_some(hint)
        })
        .or_else(|| app.rows.iter().position(|c| c.path == *anchor));
    let Some(from) = from else {
        return false;
    };
    mouse.anchor_at = from;
    let to = if event.row < area.y {
        app.offset.saturating_sub(1)
    } else if event.row >= area.y + area.height {
        app.offset + app.rendered.len()
    } else {
        // Below the last row of a short list, the drag reaches it.
        app.offset + usize::from(event.row - area.y).min(app.rendered.len().saturating_sub(1))
    }
    .min(last);
    if to == from && app.marks.is_empty() {
        return false;
    }
    let rows: Vec<PathBuf> = if from <= to {
        app.rows[from..=to].iter().map(|c| c.path.clone()).collect()
    } else {
        app.rows[to..=from]
            .iter()
            .rev()
            .map(|c| c.path.clone())
            .collect()
    };
    app.highlight = rows.last().cloned();
    app.highlight_top = false;
    app.wanted = None;
    // The view follows the highlight past the edge.
    app.scrolled = None;
    let changed =
        rows.len() != app.marks.len() || rows.iter().zip(&app.marks[..]).any(|(a, b)| a != b);
    app.marks = rows.into();
    // A drag is not the first half of a double click.
    mouse.click = None;
    changed
}
/// The button bar (`[ui] buttons`): each button does what its key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Button {
    /// Ctrl+o: the previous directory of this tab.
    Back,
    /// Alt+h: the parent directory.
    Up,
    /// l: enter the highlighted directory or open the highlighted file.
    Open,
    /// Shift+Space without moving: mark or unmark the highlighted row.
    Mark,
    /// Alt+.: show or hide hidden entries.
    Hidden,
    /// Ctrl+c.
    Cancel,
    /// Ctrl+w with marks where they are returned, else Normal-mode Enter on
    /// the highlighted row.
    Select,
    /// Esc in a confirmation.
    Dismiss,
    /// Enter in a confirmation.
    Confirm,
}
impl Button {
    /// The buttons for `focus` (left and right groups, left to right); `None`
    /// hides the bar. A narrow bar drops buttons from the end of the first
    /// group, never Cancel or Select.
    pub(crate) fn sets(focus: &Focus) -> Option<(&'static [Self], &'static [Self])> {
        match focus {
            Focus::Results => Some((
                &[Self::Back, Self::Up, Self::Open, Self::Mark, Self::Hidden],
                &[Self::Cancel, Self::Select],
            )),
            Focus::TransferConfirm(_) | Focus::DeleteConfirm(_) => {
                Some((&[], &[Self::Dismiss, Self::Confirm]))
            }
            _ => None,
        }
    }
    /// The key a button stands for, which the session handles as that key;
    /// Select in a file dialog is Enter on the highlighted row (unless it
    /// returns marks), so saving over a file asks and folders are entered.
    pub(crate) fn key(self, app: &App) -> Option<KeyEvent> {
        match self {
            Self::Dismiss => Some(KeyEvent::new(K::Esc, M::NONE)),
            Self::Confirm => Some(KeyEvent::new(K::Enter, M::NONE)),
            Self::Select if app.dialog.is_some() && !select_returns_marks(app) => {
                Some(KeyEvent::new(K::Enter, M::NONE))
            }
            _ => None,
        }
    }
    pub(crate) fn label(self, app: &App) -> &'static str {
        match self {
            Self::Back => "← Back",
            Self::Up => "↑ Up",
            Self::Open => "Open",
            Self::Mark => match &app.highlight {
                Some(path) if app.marks.contains(path) => "Unmark",
                _ => "Mark",
            },
            Self::Hidden if app.policy.hidden => "Hide hidden",
            Self::Hidden => "Show hidden",
            Self::Cancel | Self::Dismiss => "Cancel",
            Self::Select => "Select & Exit",
            Self::Confirm => "Confirm",
        }
    }
    /// Unavailable buttons are drawn dimmed and ignore clicks.
    pub(crate) fn available(self, app: &App) -> bool {
        match self {
            Self::Back => !app.no_dir && !app.back.is_empty(),
            Self::Up => !app.no_dir && app.cwd.parent().is_some(),
            Self::Open | Self::Mark => app.highlight.is_some(),
            Self::Select => !app.marks.is_empty() || app.highlight.is_some(),
            Self::Hidden | Self::Cancel | Self::Dismiss | Self::Confirm => true,
        }
    }
}
/// A click on `button`; `Some` ends the session.
pub(super) fn press(app: &mut App, discovery: &mut Discovery, button: Button) -> Option<Finish> {
    if !button.available(app) {
        return None;
    }
    match button {
        Button::Back => {
            if app.can_navigate() && app.jump(true) {
                discovery.start(app, true);
            }
        }
        Button::Up => {
            if app.can_navigate()
                && let Some(parent) = app.cwd.parent().map(PathBuf::from)
            {
                app.navigate(parent, true);
                discovery.start(app, true);
            }
        }
        Button::Open => {
            discovery.checkpoint(app);
            activate_highlighted(app, discovery);
        }
        Button::Mark => {
            if let Some(path) = app.highlight.clone() {
                app.toggle(path);
            }
        }
        Button::Hidden => {
            app.policy.hidden = !app.policy.hidden;
            discovery.start(app, false);
        }
        Button::Cancel => return Some(Finish::Cancelled),
        // Delivered as their keys by the session.
        Button::Dismiss | Button::Confirm => {}
        Button::Select => {
            if select_returns_marks(app) {
                return leave_with_marks(app);
            }
            // Otherwise like Normal-mode Enter (in a dialog, `key` made it
            // Enter on the row), which leaves unless fff has a window of its
            // own.
            let target = app.highlighted_row().map(|c| crate::Candidate::clone(c));
            return finish_with(app, discovery, target.as_ref());
        }
    }
    None
}

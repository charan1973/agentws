use crate::discovery::Repo;
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use std::collections::HashSet;
use std::io::{self, Stdout};
use std::time::Duration;

type Term = Terminal<CrosstermBackend<Stdout>>;

/// An item renderable + selectable in the fuzzy multi-select picker.
///
/// Generic so the same UI serves repo selection (`new`) and workspace-name
/// selection (`delete`): the fuzzy match runs against `label`, identity is `key`,
/// and `detail` is extra non-matched text (e.g. a repo's path).
pub trait Pickable {
    fn key(&self) -> String;
    fn label(&self) -> String;
    fn detail(&self) -> String {
        String::new()
    }
}

impl Pickable for Repo {
    fn key(&self) -> String {
        self.path.to_string_lossy().to_string()
    }
    fn label(&self) -> String {
        self.name.clone()
    }
    fn detail(&self) -> String {
        self.path.display().to_string()
    }
}

impl Pickable for String {
    fn key(&self) -> String {
        self.clone()
    }
    fn label(&self) -> String {
        self.clone()
    }
}

struct PickerState<T: Pickable> {
    items: Vec<T>,
    filter: String,
    selected: HashSet<String>,
    cursor: usize,
}

impl<T: Pickable> PickerState<T> {
    fn new(items: Vec<T>) -> Self {
        Self {
            items,
            filter: String::new(),
            selected: HashSet::new(),
            cursor: 0,
        }
    }

    /// Items matching the current filter, best score first.
    fn filtered(&self) -> Vec<&T> {
        let matcher = SkimMatcherV2::default();
        let mut scored: Vec<(i64, &T)> = self
            .items
            .iter()
            .filter_map(|it| {
                if self.filter.is_empty() {
                    return Some((0, it));
                }
                matcher
                    .fuzzy_match(&it.label(), &self.filter)
                    .map(|s| (s, it))
            })
            .collect();
        if !self.filter.is_empty() {
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.label().cmp(&b.1.label())));
        }
        scored.into_iter().map(|(_, it)| it).collect()
    }

    fn move_cursor(&mut self, delta: i32) {
        let n = self.filtered().len();
        if n == 0 {
            self.cursor = 0;
            return;
        }
        let mut c = self.cursor as i32 + delta;
        if c < 0 {
            c = 0;
        }
        if c >= n as i32 {
            c = n as i32 - 1;
        }
        self.cursor = c as usize;
    }

    fn toggle_selected(&mut self) {
        let key = {
            let f = self.filtered();
            f.get(self.cursor).map(|it| it.key())
        };
        if let Some(key) = key {
            if !self.selected.insert(key.clone()) {
                self.selected.remove(&key);
            }
        }
    }

    fn chosen(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.items
            .iter()
            .filter(|it| self.selected.contains(&it.key()))
            .cloned()
            .collect()
    }
}

/// Full-screen fuzzy multi-select over repos (used by `new`).
/// Returns `None` if cancelled or empty.
pub fn pick(items: Vec<Repo>) -> Result<Option<Vec<Repo>>> {
    pick_items(items, "repos", "Repos")
}

/// Full-screen fuzzy multi-select over arbitrary strings (used by `delete`).
pub fn pick_strings(items: Vec<String>) -> Result<Option<Vec<String>>> {
    pick_items(items, "workspaces", "Workspaces")
}

/// Full-screen fuzzy multi-select for any item implementing [`Pickable`].
pub fn pick_items<T: Pickable + Clone>(
    items: Vec<T>,
    noun_lower: &str,
    title: &str,
) -> Result<Option<Vec<T>>> {
    if items.is_empty() {
        return Ok(None);
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = PickerState::new(items);
    let result = run(&mut terminal, &mut state, noun_lower, title);

    // Always restore the terminal.
    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();
    result
}

fn run<T: Pickable + Clone>(
    terminal: &mut Term,
    state: &mut PickerState<T>,
    noun_lower: &str,
    title: &str,
) -> Result<Option<Vec<T>>> {
    loop {
        terminal.draw(|f| ui(f, state, noun_lower, title))?;

        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(None);
            }
            KeyCode::Esc => return Ok(None),
            KeyCode::Enter => {
                let chosen = state.chosen();
                return Ok(if chosen.is_empty() {
                    None
                } else {
                    Some(chosen)
                });
            }
            KeyCode::Down | KeyCode::Char('j') => state.move_cursor(1),
            KeyCode::Up | KeyCode::Char('k') => state.move_cursor(-1),
            KeyCode::Char(' ') | KeyCode::Tab => state.toggle_selected(),
            KeyCode::Backspace => {
                state.filter.pop();
                state.cursor = 0;
            }
            KeyCode::Char(c) if !c.is_control() => {
                state.filter.push(c);
                state.cursor = 0;
            }
            _ => {}
        }
    }
}

fn ui<T: Pickable>(frame: &mut Frame, state: &mut PickerState<T>, noun_lower: &str, title: &str) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    // Input box.
    let input = Paragraph::new(format!(" {}", state.filter)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Filter (type to fuzzy-match {noun_lower})")),
    );
    frame.render_widget(input, chunks[0]);

    // Build list items from the filtered set.
    let (list_items, count) = {
        let filtered = state.filtered();
        let count = filtered.len();
        let items: Vec<ListItem> = filtered
            .iter()
            .map(|it| {
                let check = if state.selected.contains(&it.key()) {
                    "[x]"
                } else {
                    "[ ]"
                };
                let detail = it.detail();
                let line = if detail.is_empty() {
                    format!(" {} {}", check, it.label())
                } else {
                    format!(" {} {:<26} {}", check, it.label(), detail)
                };
                ListItem::new(Line::raw(line))
            })
            .collect();
        (items, count)
    };

    if count > 0 && state.cursor >= count {
        state.cursor = count - 1;
    }

    let mut list_state = ListState::default();
    list_state.select(if count == 0 { None } else { Some(state.cursor) });

    let list = List::new(list_items)
        .block(Block::default().borders(Borders::ALL).title(format!(
            "{title} ({count}) — ↑/↓ move · Space toggle · Enter confirm · Esc cancel"
        )))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, chunks[1], &mut list_state);

    // Footer.
    let footer = Paragraph::new(format!(
        " {} selected of {} total",
        state.selected.len(),
        state.items.len()
    ));
    frame.render_widget(footer, chunks[2]);
}

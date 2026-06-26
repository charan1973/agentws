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

struct PickerState {
    items: Vec<Repo>,
    filter: String,
    selected: HashSet<String>,
    cursor: usize,
}

impl PickerState {
    fn new(items: Vec<Repo>) -> Self {
        Self {
            items,
            filter: String::new(),
            selected: HashSet::new(),
            cursor: 0,
        }
    }

    fn key_of(r: &Repo) -> String {
        r.path.to_string_lossy().to_string()
    }

    /// Repos matching the current filter, best score first.
    fn filtered(&self) -> Vec<&Repo> {
        let matcher = SkimMatcherV2::default();
        let mut scored: Vec<(i64, &Repo)> = self
            .items
            .iter()
            .filter_map(|r| {
                if self.filter.is_empty() {
                    return Some((0, r));
                }
                matcher.fuzzy_match(&r.name, &self.filter).map(|s| (s, r))
            })
            .collect();
        if !self.filter.is_empty() {
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        }
        scored.into_iter().map(|(_, r)| r).collect()
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
            f.get(self.cursor).map(|r| PickerState::key_of(r))
        };
        if let Some(key) = key {
            if !self.selected.insert(key.clone()) {
                self.selected.remove(&key);
            }
        }
    }

    fn chosen(&self) -> Vec<Repo> {
        self.items
            .iter()
            .filter(|r| self.selected.contains(&Self::key_of(r)))
            .cloned()
            .collect()
    }
}

/// Open a full-screen fuzzy multi-select. Returns `None` if cancelled or empty.
pub fn pick(items: Vec<Repo>) -> Result<Option<Vec<Repo>>> {
    if items.is_empty() {
        return Ok(None);
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = PickerState::new(items);
    let result = run(&mut terminal, &mut state);

    // Always restore the terminal.
    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();
    result
}

fn run(terminal: &mut Term, state: &mut PickerState) -> Result<Option<Vec<Repo>>> {
    loop {
        terminal.draw(|f| ui(f, state))?;

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
                return Ok(if chosen.is_empty() { None } else { Some(chosen) });
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

fn ui(frame: &mut Frame, state: &mut PickerState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    // Input box.
    let input = Paragraph::new(format!(" {}", state.filter))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Filter (type to fuzzy-match repos)"),
        );
    frame.render_widget(input, chunks[0]);

    // Build list items from the filtered set.
    let (list_items, count) = {
        let filtered = state.filtered();
        let count = filtered.len();
        let items: Vec<ListItem> = filtered
            .iter()
            .map(|r| {
                let check = if state.selected.contains(&PickerState::key_of(r)) {
                    "[x]"
                } else {
                    "[ ]"
                };
                ListItem::new(Line::raw(format!(
                    " {} {:<26} {}",
                    check,
                    r.name,
                    r.path.display()
                )))
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
        .block(
            Block::default().borders(Borders::ALL).title(format!(
                "Repos ({count}) — ↑/↓ move · Space toggle · Enter confirm · Esc cancel"
            )),
        )
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

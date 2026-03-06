/*
 * szsol-rs
 * Copyright (C) 2026 ghoker143
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * RELICENSING NOTICE:
 * This project was originally released under the MIT License. As of March 2026, 
 * the sole copyright holder (ghoker143) has officially transitioned the 
 * entire project and its history to the GNU General Public License v3.0.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */
use std::io::{self, BufRead, Write};
use std::time::{Duration, Instant};

use crossterm::event::{self as ct_event, Event};


use crate::board::{Board, Location};
use crate::command::{parse_command, Command};
use crate::renderer::Renderer;
use crate::history::{History, GameRecord};


/// The main game loop.  `renderer` is injected so the engine stays
/// renderer-agnostic (CLI today, TUI tomorrow).
pub struct Game<R: Renderer> {
    board: Board,
    renderer: R,
    history: Vec<Board>, // for undo
    save_data: History,
    should_quit: bool,
    last_tui_click: Option<(Location, Instant)>,
}


impl<R: Renderer> Game<R> {
    pub fn init(seed: Option<u64>, mut renderer: R) -> Self {
        let mut save_data = History::load();
        
        // 1. Check if we can resume the last game
        let mut resumed_board = None;
        let mut resumed_history = Vec::new();
        let mut abandon_old = false;

        if let Some(last) = save_data.records.last_mut() {
            if last.end_time.is_none() {
                // Determine if we should resume or abandon
                if seed.is_none() || seed == Some(last.seed) {
                    if let Some(cb) = &last.current_board {
                        resumed_board = Some(cb.clone());
                        resumed_history = last.undo_history.clone();
                        renderer.info(&format!("Resumed game from seed {}.", last.seed));
                    } else {
                        abandon_old = true;
                    }
                } else {
                    // Given a new distinct seed, so we abandon the old unfinished run.
                    abandon_old = true;
                }
            }
        }

        if abandon_old {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            if let Some(last) = save_data.records.last_mut() {
                if last.end_time.is_none() {
                    last.end_time = Some(now);
                    last.current_board = None;
                    last.undo_history.clear();
                }
            }
        }

        let board = match resumed_board {
            Some(b) => b,
            None => {
                let new_board = match seed {
                    Some(s) => Board::deal_seeded(s),
                    None => Board::deal_random(),
                };
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                let mut record = GameRecord::new(new_board.seed, now);
                record.initial_board = Some(new_board.clone());
                record.current_board = Some(new_board.clone());
                save_data.records.push(record);
                save_data.save();
                new_board
            }
        };

        Game {
            board,
            renderer,
            history: resumed_history,
            save_data,
            should_quit: false,
            last_tui_click: None,
        }
    }


    /// Run the interactive game loop until the player quits.
    pub fn run(&mut self) {
        let stdin = io::stdin();
        let mut stdout = io::stdout();

        // Auto-move any immediately playable cards on deal.
        let (n, events) = self.board.auto_move();
        self.renderer.push_events(events);
        if n > 0 {
            self.renderer.info(&format!("Auto-moved {} card(s) to foundation.", n));
        }

        self.renderer.render_header(self.save_data.total_wins(), self.board.seed);
        self.renderer.render(&self.board);

        loop {
            print!("> ");
            stdout.flush().unwrap();

            let mut line = String::new();
            if stdin.lock().read_line(&mut line).unwrap() == 0 {
                if let Some(last) = self.save_data.records.last_mut() {
                    last.current_board = Some(self.board.clone());
                    last.undo_history = self.history.clone();
                }
                self.save_data.save();
                break;
            }

            match parse_command(&line) {
                Err(e) => self.renderer.error(&e),
                Ok(cmd) => {
                    let quit = self.handle(cmd);
                    if quit {
                        break;
                    }

                    // Auto-move after every successful command.
                    let (n, events) = self.board.auto_move();
                    self.renderer.push_events(events);
                    if n > 0 {
                        self.renderer
                            .info(&format!("Auto-moved {} card(s) to foundation.", n));
                    }


                    // Save progress to disk for resuming
                    if let Some(last) = self.save_data.records.last_mut() {
                        last.current_board = Some(self.board.clone());
                        last.undo_history = self.history.clone();
                    }
                    self.save_data.save();

                    if self.board.is_won() {
                        self.record_win();
                        self.renderer.win();
                        // Handle post-win input (like typing "new" to deal another hand)
                        self.renderer.render_header(self.save_data.total_wins(), self.board.seed);
                        self.renderer.render(&self.board);
                        continue;
                    }

                    self.renderer.render_header(self.save_data.total_wins(), self.board.seed);
                    self.renderer.render(&self.board);
                }
            }
        }
    }

    /// TUI tick-driven loop with direct keybinding → SelectionState dispatch.
    pub fn run_tui(&mut self)
    where
        R: crate::tui_renderer::TuiRendererExt,
    {
        // Initial auto-move + render
        self.renderer.info("Press ? for help.");
        let (n, events) = self.board.auto_move();
        self.renderer.push_events(events);
        if n > 0 {
            self.renderer.info(&format!("Auto-moved {} card(s) to foundation.", n));
        }
        self.renderer.render_header(self.save_data.total_wins(), self.board.seed);
        self.renderer.render(&self.board);

        loop {
            if ct_event::poll(Duration::from_millis(16)).unwrap_or(false) {
                match ct_event::read() {
                    Ok(Event::Key(key)) => {
                        self.handle_tui_key(key);
                    }
                    Ok(Event::Mouse(me)) => {
                        self.handle_tui_mouse(me);
                    }
                    _ => {}
                }
            }

            if self.should_quit { break; }

            self.renderer.tick();
            self.renderer.render_header(self.save_data.total_wins(), self.board.seed);
            self.renderer.render(&self.board);
        }
    }

    /// Process a single key event in TUI mode.
    fn handle_tui_key(&mut self, key: crossterm::event::KeyEvent)
    where
        R: crate::tui_renderer::TuiRendererExt,
    {
        use crossterm::event::{KeyCode, KeyModifiers};
        use crate::tui_renderer::{SelectionState, COL_KEYS, FC_KEYS};
        use crate::board::Location;
        use crate::card::Suit;

        // Ctrl-C / Ctrl-D = hard quit
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') | KeyCode::Char('d') => { self.should_quit = true; return; }
                _ => {}
            }
        }

        let c = match key.code {
            KeyCode::Char(c) => c,
            KeyCode::Enter => {
                // Move selected card/stack to foundation
                self.tui_move_to_foundation();
                return;
            }
            KeyCode::Esc => {
                self.renderer.set_selection(SelectionState::Idle);
                return;
            }
            _ => return,
        };

        let sel = self.renderer.get_selection().clone();

        match &sel {
            // ── Idle: interpret key as source selection ──────────────────
            SelectionState::Idle => {
                if c == 'd' || c == 'D' {
                    self.renderer.set_selection(SelectionState::WaitDragonSuit);
                } else if c == 'z' || c == 'Z' {
                    // Undo
                    if let Some(prev) = self.history.pop() {
                        self.board = prev;
                        self.renderer.clear_status_log();
                        self.renderer.info("Undo.");
                    } else {
                        self.renderer.error("Nothing to undo.");
                    }
                } else if c == 'n' || c == 'N' {
                    self.tui_new_game();
                } else if c == '?' {
                    self.renderer.toggle_help();
                } else if let Some(col) = COL_KEYS.iter().position(|&k| k == c) {
                    if !self.board.columns[col].is_empty() {
                        self.renderer.set_selection(SelectionState::Column { col, depth: 1 });
                    }
                } else if let Some(fc) = FC_KEYS.iter().position(|&k| k == c) {
                    if self.board.free_cells[fc].card().is_some() {
                        self.renderer.set_selection(SelectionState::FreeCell { idx: fc });
                    }
                }
            }

            // ── Dragon suit selection ────────────────────────────────────
            SelectionState::WaitDragonSuit => {
                let suit = match c {
                    'r' | 'R' => Some(Suit::Red),
                    'g' | 'G' => Some(Suit::Green),
                    'b' | 'B' => Some(Suit::Black),
                    _ => None,
                };
                if let Some(suit) = suit {
                    self.save_history();
                    match self.board.merge_dragons(suit) {
                        Ok(events) => {
                            self.renderer.push_events(events);
                            self.tui_post_move();
                        }
                        Err(e) => {
                            self.renderer.error(e);
                            self.history.pop();
                        }
                    }
                }
                self.renderer.set_selection(SelectionState::Idle);
            }

            // ── Column selected: handle second key ───────────────────────
            SelectionState::Column { col, depth } => {
                let col = *col;
                let depth = *depth;

                // Same column key again → try to extend selection upward
                if COL_KEYS.get(col) == Some(&c) {
                    let col_len = self.board.columns[col].len();
                    let next_start = col_len.saturating_sub(depth + 1);
                    let max_stack = self.board.stack_len(col, next_start);
                    if max_stack >= depth + 1 {
                        // Can extend one more card
                        self.renderer.set_selection(SelectionState::Column { col, depth: depth + 1 });
                    } else {
                        // Already at maximum valid stack — cycle back to 1
                        self.renderer.set_selection(SelectionState::Column { col, depth: 1 });
                    }
                    return;
                }

                // Target is another column
                if let Some(dst_col) = COL_KEYS.iter().position(|&k| k == c) {
                    let col_len = self.board.columns[col].len();
                    let start_idx = col_len.saturating_sub(depth);
                    self.save_history();
                    match self.board.move_stack(col, start_idx, dst_col) {
                        Ok(()) => { self.tui_post_move(); }
                        Err(e) => {
                            self.renderer.error(e);
                            self.history.pop();
                        }
                    }
                    self.renderer.set_selection(SelectionState::Idle);
                    return;
                }

                // Target is a free cell (only depth==1 allowed)
                if let Some(dst_fc) = FC_KEYS.iter().position(|&k| k == c) {
                    if depth == 1 {
                        let src = Location::Column(col);
                        let dst = Location::FreeCell(dst_fc);
                        self.save_history();
                        match self.board.move_card(src, dst) {
                            Ok(events) => {
                                self.renderer.push_events(events);
                                self.tui_post_move();
                            }
                            Err(e) => {
                                self.renderer.error(e);
                                self.history.pop();
                            }
                        }
                    } else {
                        self.renderer.error("只能移动单张牌到空格。");
                    }
                    self.renderer.set_selection(SelectionState::Idle);
                    return;
                }

                // 'n' / 'z' etc. still work even when something is selected
                if c == 'z' || c == 'Z' {
                    if let Some(prev) = self.history.pop() {
                        self.board = prev;
                        self.renderer.clear_status_log();
                        self.renderer.info("Undo.");
                    }
                    self.renderer.set_selection(SelectionState::Idle);
                }
            }

            // ── Free cell selected ────────────────────────────────────────
            SelectionState::FreeCell { idx } => {
                let idx = *idx;

                // Target column
                if let Some(dst_col) = COL_KEYS.iter().position(|&k| k == c) {
                    let src = Location::FreeCell(idx);
                    let dst = Location::Column(dst_col);
                    self.save_history();
                    match self.board.move_card(src, dst) {
                        Ok(events) => {
                            self.renderer.push_events(events);
                            self.tui_post_move();
                        }
                        Err(e) => {
                            self.renderer.error(e);
                            self.history.pop();
                        }
                    }
                    self.renderer.set_selection(SelectionState::Idle);
                    return;
                }

                // Same FC key = deselect
                if FC_KEYS.get(idx) == Some(&c) {
                    self.renderer.set_selection(SelectionState::Idle);
                    return;
                }

                // z = undo
                if c == 'z' || c == 'Z' {
                    if let Some(prev) = self.history.pop() {
                        self.board = prev;
                        self.renderer.clear_status_log();
                        self.renderer.info("Undo.");
                    }
                    self.renderer.set_selection(SelectionState::Idle);
                }
            }
        }
    }

    /// Handle mouse click in TUI mode.
    fn handle_tui_mouse(&mut self, me: crossterm::event::MouseEvent)
    where
        R: crate::tui_renderer::TuiRendererExt,
    {
        use crossterm::event::MouseEventKind;
        use crate::tui_renderer::{SelectionState, CARD_PEEK_ROWS};

        if me.kind != MouseEventKind::Down(crossterm::event::MouseButton::Left) {
            return;
        }
        // hit-test against the last rendered layout
        if let Some(loc) = self.renderer.hit_test(me.column, me.row) {
            if self.tui_handle_double_click(loc, me.row) {
                return;
            }

            let sel = self.renderer.get_selection().clone();
            match sel {
                SelectionState::Idle => {
                    // Select the clicked location
                    match loc {
                        crate::board::Location::Column(col) if !self.board.columns[col].is_empty() => {
                            let depth = self.tui_click_column_depth(col, me.row, CARD_PEEK_ROWS as u16);
                            self.renderer.set_selection(SelectionState::Column { col, depth });
                        }
                        crate::board::Location::FreeCell(idx) if self.board.free_cells[idx].card().is_some() => {
                            self.renderer.set_selection(SelectionState::FreeCell { idx });
                        }
                        _ => {}
                    }
                }
                SelectionState::Column { col: src_col, depth } => {
                    let col_len = self.board.columns[src_col].len();
                    let start_idx = col_len.saturating_sub(depth);
                    match loc {
                        crate::board::Location::Column(dst_col) if dst_col != src_col => {
                            self.save_history();
                            if let Err(e) = self.board.move_stack(src_col, start_idx, dst_col) {
                                self.renderer.error(e);
                                self.history.pop();
                            } else {
                                self.tui_post_move();
                            }
                            self.renderer.set_selection(SelectionState::Idle);
                        }
                        crate::board::Location::Foundation(suit) if depth == 1 => {
                            let _ = suit;
                            self.renderer.set_selection(SelectionState::Column { col: src_col, depth });
                            self.tui_move_to_foundation();
                        }
                        crate::board::Location::FreeCell(dst_fc) if depth == 1 => {
                            let src = crate::board::Location::Column(src_col);
                            let dst = crate::board::Location::FreeCell(dst_fc);
                            self.save_history();
                            match self.board.move_card(src, dst) {
                                Ok(events) => { self.renderer.push_events(events); self.tui_post_move(); }
                                Err(e) => { self.renderer.error(e); self.history.pop(); }
                            }
                            self.renderer.set_selection(SelectionState::Idle);
                        }
                        _ => { self.renderer.set_selection(SelectionState::Idle); }
                    }
                }
                SelectionState::FreeCell { idx: src_fc } => {
                    match loc {
                        crate::board::Location::Column(dst_col) => {
                            let src = crate::board::Location::FreeCell(src_fc);
                            let dst = crate::board::Location::Column(dst_col);
                            self.save_history();
                            match self.board.move_card(src, dst) {
                                Ok(events) => { self.renderer.push_events(events); self.tui_post_move(); }
                                Err(e) => { self.renderer.error(e); self.history.pop(); }
                            }
                            self.renderer.set_selection(SelectionState::Idle);
                        }
                        crate::board::Location::Foundation(suit) => {
                            let _ = suit;
                            self.renderer.set_selection(SelectionState::FreeCell { idx: src_fc });
                            self.tui_move_to_foundation();
                        }
                        _ => {
                            self.renderer.set_selection(SelectionState::Idle);
                        }
                    }
                }
                _ => { self.renderer.set_selection(SelectionState::Idle); }
            }
        }
    }

    fn tui_handle_double_click(&mut self, loc: Location, row: u16) -> bool
    where
        R: crate::tui_renderer::TuiRendererExt,
    {
        let now = Instant::now();
        let is_double = self
            .last_tui_click
            .map(|(last_loc, at)| last_loc == loc && now.duration_since(at) <= Duration::from_millis(350))
            .unwrap_or(false);
        self.last_tui_click = Some((loc, now));

        if !is_double {
            return false;
        }

        let suit = match loc {
            Location::Column(col) => {
                let depth = self.tui_click_column_depth(col, row, crate::tui_renderer::CARD_PEEK_ROWS as u16);
                let idx = self.board.columns[col].len().saturating_sub(depth);
                match self.board.columns[col].get(idx).copied() {
                    Some(crate::card::Card::Dragon(suit)) => Some(suit),
                    _ => None,
                }
            }
            Location::FreeCell(idx) => match self.board.free_cells[idx].card() {
                Some(crate::card::Card::Dragon(suit)) => Some(suit),
                _ => None,
            },
            _ => None,
        };

        let Some(suit) = suit else {
            return false;
        };

        self.save_history();
        match self.board.merge_dragons(suit) {
            Ok(events) => {
                self.renderer.push_events(events);
                self.tui_post_move();
            }
            Err(e) => {
                self.renderer.error(e);
                self.history.pop();
            }
        }
        self.renderer.set_selection(crate::tui_renderer::SelectionState::Idle);
        true
    }

    fn tui_click_column_depth(&self, col: usize, click_row: u16, peek_rows: u16) -> usize
    where
        R: crate::tui_renderer::TuiRendererExt,
    {
        let cards = &self.board.columns[col];
        let len = cards.len();
        if len <= 1 {
            return len.max(1);
        }
        let Some(col_rect) = self.renderer.slot_rect(crate::board::Location::Column(col)) else {
            return 1;
        };
        let cards_start_y = col_rect.y;

        let rel = click_row.saturating_sub(cards_start_y);
        let top_card_start = (len as u16 - 1) * peek_rows;
        let from_idx = if rel >= top_card_start {
            len - 1
        } else {
            (rel / peek_rows) as usize
        };

        let valid_len = self.board.stack_len(col, from_idx);
        let requested = len - from_idx;
        requested.min(valid_len).max(1)
    }

    /// Common post-move logic in TUI: auto-move, save, win-check.
    fn tui_post_move(&mut self)
    where
        R: crate::tui_renderer::TuiRendererExt,
    {
        self.renderer.clear_status_log();
        let (n, events) = self.board.auto_move();
        self.renderer.push_events(events);
        if n > 0 {
            self.renderer.info(&format!("Auto-moved {} card(s).", n));
        }
        if let Some(last) = self.save_data.records.last_mut() {
            last.current_board = Some(self.board.clone());
            last.undo_history = self.history.clone();
        }
        self.save_data.save();
        if self.board.is_won() {
            self.record_win();
            self.renderer.win();
        }
    }

    /// Move selected card/stack to foundation directly (Enter key).
    fn tui_move_to_foundation(&mut self)
    where
        R: crate::tui_renderer::TuiRendererExt,
    {
        use crate::tui_renderer::SelectionState;
        let sel = self.renderer.get_selection().clone();
        let src = match sel {
            SelectionState::Column { col, depth: 1 } => Some(Location::Column(col)),
            SelectionState::FreeCell { idx }          => Some(Location::FreeCell(idx)),
            _ => None,
        };
        if let Some(src) = src {
            self.save_history();
            match self.board.move_to_foundation(src) {
                Ok(events) => {
                    self.renderer.push_events(events);
                    self.tui_post_move();
                }
                Err(e) => {
                    self.renderer.error(e);
                    self.history.pop();
                }
            }
        }
        self.renderer.set_selection(SelectionState::Idle);
    }

    /// Start a new game in TUI mode.
    fn tui_new_game(&mut self)
    where
        R: crate::tui_renderer::TuiRendererExt,
    {
        self.record_abandon();
        self.board = Board::deal_random();
        self.history.clear();

        let initial_board = self.board.clone();
        let (n, events) = self.board.auto_move();
        self.renderer.push_events(events);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default().as_secs() as i64;
        let mut record = crate::history::GameRecord::new(self.board.seed, now);
        record.initial_board = Some(initial_board);
        record.current_board = Some(self.board.clone());
        self.save_data.records.push(record);
        self.save_data.save();
        self.renderer.clear_status_log();
        self.renderer.info("New game dealt.");
        if n > 0 {
            self.renderer.info(&format!("Auto-moved {} card(s) to foundation.", n));
        }
        self.renderer.set_selection(crate::tui_renderer::SelectionState::Idle);
    }



    
    fn record_abandon(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
            
        if let Some(last) = self.save_data.records.last_mut() {
            if last.end_time.is_none() {
                last.end_time = Some(now);
                last.current_board = None;
                last.undo_history.clear();
                self.save_data.save();
            }
        }
    }

    fn record_win(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
            
        if let Some(last) = self.save_data.records.last_mut() {
            if !last.won {
                last.end_time = Some(now);
                last.won = true;
                last.current_board = None;
                last.undo_history.clear();
                self.save_data.save();
            }
        }
    }

    /// Dispatch a command.  Returns `true` if the game should exit.
    fn handle(&mut self, cmd: Command) -> bool {
        match cmd {
            Command::Quit => {
                // Do not mark as abandoned, so it can be resumed. Just save current state.
                if let Some(last) = self.save_data.records.last_mut() {
                    last.current_board = Some(self.board.clone());
                    last.undo_history = self.history.clone();
                }
                self.save_data.save();
                
                self.renderer.info("Thanks for playing. Goodbye!");
                return true;
            }
            Command::Help => {
                self.renderer.help();
            }
            Command::NewGame => {
                self.record_abandon(); // Finish the previous game
                
                self.board = Board::deal_random();
                self.history.clear();
                
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                self.save_data.records.push(GameRecord::new(self.board.seed, now));
                self.save_data.save();
                
                self.renderer.info("A new game has been dealt.");
            }
            Command::Undo => {
                if let Some(prev) = self.history.pop() {
                    self.board = prev;
                    self.renderer.info("Undo successful.");
                } else {
                    self.renderer.error("Nothing to undo.");
                }
            }
            Command::Solve => {
                self.renderer.info("Running A* solver... (may take a moment)");
                
                if let Some(path) = crate::solver::solve(&self.board) {
                    self.renderer.info(&format!("Found a solution in {} steps!", path.len()));
                    for (i, m) in path.iter().enumerate() {
                        self.renderer.info(&format!("{:4}. {}", i + 1, m.to_command_str()));
                    }
                } else {
                    self.renderer.error("No solution found by BFS.");
                }
            }
            Command::ColumnToColumn { src, stack_start, dst } => {
                self.save_history();
                let col_len = self.board.columns[src].len();
                // stack_start is depth from top; convert to absolute index.
                let abs_idx = if col_len == 0 {
                    self.renderer.error("Source column is empty.");
                    self.history.pop();
                    return false;
                } else {
                    col_len.saturating_sub(1 + stack_start)
                };

                match self.board.move_stack(src, abs_idx, dst) {
                    Ok(()) => {}
                    Err(e) => {
                        self.renderer.error(e);
                        self.history.pop();
                    }
                }
            }
            Command::ColumnToFreeCell { src_col, dst_cell } => {
                self.save_history();
                let src = Location::Column(src_col);
                let dst = Location::FreeCell(dst_cell);
                if let Err(e) = self.board.move_card(src, dst) {
                    self.renderer.error(e);
                    self.history.pop();
                }
            }
            Command::FreeCellToColumn { src_cell, dst_col } => {
                self.save_history();
                let src = Location::FreeCell(src_cell);
                let dst = Location::Column(dst_col);
                if let Err(e) = self.board.move_card(src, dst) {
                    self.renderer.error(e);
                    self.history.pop();
                }
            }
            Command::ColumnToFoundation { src } => {
                self.save_history();
                if let Err(e) = self.board.move_to_foundation(Location::Column(src)) {
                    self.renderer.error(e);
                    self.history.pop();
                }
            }
            Command::FreeCellToFoundation { src_cell } => {
                self.save_history();
                if let Err(e) = self.board.move_to_foundation(Location::FreeCell(src_cell)) {
                    self.renderer.error(e);
                    self.history.pop();
                }
            }
            Command::MergeDragons { suit } => {
                self.save_history();
                if let Err(e) = self.board.merge_dragons(suit) {
                    self.renderer.error(e);
                    self.history.pop();
                }
            }
        }
        false
    }

    fn save_history(&mut self) {
        self.history.push(self.board.clone());
        // Cap history at 64 steps to bound memory usage.
        if self.history.len() > 64 {
            self.history.remove(0);
        }
    }
}

use std::io::{self, BufRead, Write};

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
        }
    }

    /// Run the interactive game loop until the player quits.
    pub fn run(&mut self) {
        let stdin = io::stdin();
        let mut stdout = io::stdout();

        // Auto-move any immediately playable cards on deal.
        let n = self.board.auto_move();
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
                    let n = self.board.auto_move();
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

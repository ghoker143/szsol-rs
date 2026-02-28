use std::io::{self, BufRead, Write};

use crate::board::{Board, Location};
use crate::command::{parse_command, Command};
use crate::renderer::Renderer;

/// The main game loop.  `renderer` is injected so the engine stays
/// renderer-agnostic (CLI today, TUI tomorrow).
pub struct Game<R: Renderer> {
    board: Board,
    renderer: R,
    history: Vec<Board>, // for undo
}

impl<R: Renderer> Game<R> {
    pub fn new(board: Board, renderer: R) -> Self {
        Game {
            board,
            renderer,
            history: Vec::new(),
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
        self.renderer.render(&self.board);

        loop {
            print!("> ");
            stdout.flush().unwrap();

            let mut line = String::new();
            if stdin.lock().read_line(&mut line).unwrap() == 0 {
                // EOF
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

                    if self.board.is_won() {
                        self.renderer.win();
                        // Offer a new game.
                        self.renderer.render(&self.board);
                        continue;
                    }

                    self.renderer.render(&self.board);
                }
            }
        }
    }

    /// Dispatch a command.  Returns `true` if the game should exit.
    fn handle(&mut self, cmd: Command) -> bool {
        match cmd {
            Command::Quit => {
                self.renderer.info("Thanks for playing. Goodbye!");
                return true;
            }
            Command::Help => {
                self.renderer.help();
            }
            Command::NewGame => {
                self.board = Board::deal_random();
                self.history.clear();
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

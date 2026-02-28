/// Trait that abstracts the rendering layer.
///
/// Implement this trait for:
/// - `CliRenderer` – plain terminal output (current implementation)
/// - `TuiRenderer` – ratatui-based full-screen TUI (future)
pub trait Renderer {
    /// Render the full game board.
    fn render(&mut self, board: &crate::board::Board);
    /// Display an informational message.
    fn info(&mut self, msg: &str);
    /// Display an error message.
    fn error(&mut self, msg: &str);
    /// Display the help text.
    fn help(&mut self);
    /// Display the win screen.
    fn win(&mut self);
}

// ---------------------------------------------------------------------------
// CLI Renderer
// ---------------------------------------------------------------------------

/// A simple ANSI-color CLI renderer.
pub struct CliRenderer;

impl CliRenderer {
    pub fn new() -> Self {
        CliRenderer
    }

    fn card_str(&self, card: crate::card::Card) -> String {
        use crate::card::{Card, Suit};
        let label = card.label();
        match card {
            Card::Numbered(Suit::Red, _) | Card::Dragon(Suit::Red) => {
                format!("\x1b[31m{}\x1b[0m", label) // red
            }
            Card::Numbered(Suit::Green, _) | Card::Dragon(Suit::Green) => {
                format!("\x1b[32m{}\x1b[0m", label) // green
            }
            Card::Numbered(Suit::Black, _) | Card::Dragon(Suit::Black) => {
                format!("\x1b[90m{}\x1b[0m", label) // dark gray
            }
            Card::Flower => format!("\x1b[35m{}\x1b[0m", label), // magenta
        }
    }

    fn freecell_str(&self, fc: &crate::board::FreeCellState) -> String {
        use crate::board::FreeCellState;
        match fc {
            FreeCellState::Empty => "   ".to_string(),
            FreeCellState::Card(c) => format!("[{}]", self.card_str(*c)),
            FreeCellState::DragonLocked(s) => {
                use crate::card::Suit;
                let label = match s {
                    Suit::Red => "\x1b[31mXXX\x1b[0m",
                    Suit::Green => "\x1b[32mXXX\x1b[0m",
                    Suit::Black => "\x1b[90mXXX\x1b[0m",
                };
                format!("[{}]", label)
            }
        }
    }
}

impl Renderer for CliRenderer {
    fn render(&mut self, board: &crate::board::Board) {
        use crate::card::Suit;

        println!();

        // ---- Top row: free cells | flower | foundations ----
        // Free cells (0–2)
        print!("  FREE CELLS:  ");
        for (i, fc) in board.free_cells.iter().enumerate() {
            print!("{}: {}  ", i, self.freecell_str(fc));
        }

        // Flower slot
        if board.flower_placed {
            print!("  FLOWER: \x1b[35m[FL]\x1b[0m  ");
        } else {
            print!("  FLOWER: [  ]  ");
        }

        // Foundations
        print!("  FOUND: ");
        for suit in &[Suit::Red, Suit::Green, Suit::Black] {
            let idx = match suit {
                Suit::Red => 0,
                Suit::Green => 1,
                Suit::Black => 2,
            };
            let v = board.foundations[idx];
            if v == 0 {
                print!("{}[--] ", suit.symbol());
            } else {
                let card = crate::card::Card::Numbered(*suit, v);
                print!("{}[{}] ", suit.symbol(), self.card_str(card));
            }
        }
        println!();

        // ---- Column indices header ----
        println!();
        print!("  COL:   ");
        for i in 0..crate::board::NUM_COLUMNS {
            print!("  {:^4}", i);
        }
        println!();

        // ---- Tableau ----
        // Find the longest column
        let max_len = board.columns.iter().map(|c| c.len()).max().unwrap_or(0);

        for row in 0..max_len {
            print!("  {:>3}:   ", row);
            for col in &board.columns {
                if row < col.len() {
                    print!(" [{}] ", self.card_str(col[row]));
                } else {
                    print!("  ..  ");
                }
            }
            println!();
        }

        if max_len == 0 {
            println!("  (all columns empty)");
        }

        println!();
    }

    fn info(&mut self, msg: &str) {
        println!("\x1b[36m[INFO]\x1b[0m {}", msg);
    }

    fn error(&mut self, msg: &str) {
        println!("\x1b[31m[ERR ]\x1b[0m {}", msg);
    }

    fn help(&mut self) {
        println!(
            r#"
╔══════════════════════════════════════════════════════════════╗
║          SHENZHEN I/O Solitaire – CLI Help                   ║
╠══════════════════════════════════════════════════════════════╣
║  GOAL: Move all numbered cards (1-9) to the foundation and   ║
║        clear the tableau.                                    ║
║                                                              ║
║  CARDS: 3 suits (Red/Green/Black), each with:                ║
║    · Numbered cards 1-9    · 4 Dragon cards (RD/GD/BD)       ║
║    · 1 Flower card (FL) shared across all suits              ║
║                                                              ║
║  RULES:                                                      ║
║    · Stack cards on columns: different suit, value - 1       ║
║      e.g. R5 can go on G6 or B6, but not R6                 ║
║    · Foundation builds up by suit: R1 → R2 → ... → R9       ║
║    · 3 Free Cells: each holds 1 card temporarily            ║
║    · Flower card goes to the flower slot (auto if exposed)   ║
║    · 4 same-color Dragons can be merged when all exposed     ║
║      (locks one free cell permanently)                       ║
╠══════════════════════════════════════════════════════════════╣
║  COMMANDS (case-insensitive):                                ║
║                                                              ║
║  cc  <src> <dst>         Move top card: column → column      ║
║  cc  <src>:<N> <dst>     Move stack of N+1 cards from top    ║
║                          (0=top card only, 1=top 2, etc.)    ║
║  cf  <col> <cell>        Move top card: column → free cell   ║
║  fc  <cell> <col>        Move card: free cell → column       ║
║  ctf <col>               Move top card: column → foundation  ║
║  ftf <cell>              Move card: free cell → foundation   ║
║  dragon r|g|b            Merge all 4 exposed dragons         ║
║  undo                    Undo last move                      ║
║  new                     Start a new random game             ║
║  quit                    Exit                                ║
║  help | h | ?            Show this help                      ║
╠══════════════════════════════════════════════════════════════╣
║  Example: cc 4:2 7  →  move top 3 cards of col 4 to col 7   ║
║                                                              ║
║  * Safe cards are moved to foundation automatically.         ║
╚══════════════════════════════════════════════════════════════╝
"#
        );
    }

    fn win(&mut self) {
        println!(
            "\n\x1b[33m\
            \n  ██╗    ██╗ ██████╗ ███╗   ██╗██╗\
            \n  ██║    ██║██╔═══██╗████╗  ██║██║\
            \n  ██║ █╗ ██║██║   ██║██╔██╗ ██║██║\
            \n  ██║███╗██║██║   ██║██║╚██╗██║╚═╝\
            \n  ╚███╔███╔╝╚██████╔╝██║ ╚████║██╗\
            \n   ╚══╝╚══╝  ╚═════╝ ╚═╝  ╚═══╝╚═╝\
            \n\x1b[0m\
            \n  Congratulations! You solved it!  Type 'new' for another game.\n"
        );
    }
}

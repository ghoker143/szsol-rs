/// All commands a player can issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Move the top card (or a stack starting at `stack_start`) from a column
    /// to another column.
    /// `stack_start`: index from the **top** of the column (0 = top card).
    ColumnToColumn {
        src: usize,
        stack_start: usize,
        dst: usize,
    },
    /// Move the top card of a column to a free cell.
    ColumnToFreeCell { src_col: usize, dst_cell: usize },
    /// Move the card in a free cell to a column.
    FreeCellToColumn { src_cell: usize, dst_col: usize },
    /// Move the top card of a column to the foundation (auto-detects flower/numbered).
    ColumnToFoundation { src: usize },
    /// Move the card in a free cell to the foundation.
    FreeCellToFoundation { src_cell: usize },
    /// Merge all four exposed dragons of a suit.
    MergeDragons { suit: crate::card::Suit },
    /// Undo the last move (optional, not yet implemented).
    Undo,
    /// Quit the game.
    Quit,
    /// Give up and start a new game.
    NewGame,
    /// Print help.
    Help,
}

/// Parse a single line of text input into a `Command`.
///
/// Syntax reference (case-insensitive):
/// ```
/// cc <src_col> <dst_col>            -- Move top card column→column
/// cc <src_col>:<depth> <dst_col>    -- Move stack column→column (0=top)
/// cf <src_col> <cell_idx>           -- Move column top → free cell
/// fc <cell_idx> <dst_col>           -- Move free cell → column
/// ctf <src_col>                     -- Move column top → foundation
/// ftf <cell_idx>                    -- Move free cell → foundation
/// dragon r|g|b                      -- Merge dragons of a suit
/// undo                              -- Undo last move
/// new                               -- New game
/// quit | q                          -- Quit
/// help | h | ?                      -- Help
/// ```
pub fn parse_command(input: &str) -> Result<Command, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Empty input".to_string());
    }

    let tokens: Vec<&str> = input.split_whitespace().collect();
    let cmd = tokens[0].to_lowercase();

    match cmd.as_str() {
        "cc" => {
            if tokens.len() < 3 {
                return Err("Usage: cc <src[:<depth>]> <dst>".to_string());
            }
            let dst: usize = parse_col_idx(tokens[2])?;
            // Parse optional stack depth: "3:2" means column 3, starting 2 from top.
            if let Some((col_part, depth_part)) = tokens[1].split_once(':') {
                let src: usize = parse_col_idx(col_part)?;
                let stack_start: usize = depth_part.parse().map_err(|_| "Invalid depth".to_string())?;
                Ok(Command::ColumnToColumn { src, stack_start, dst })
            } else {
                let src: usize = parse_col_idx(tokens[1])?;
                Ok(Command::ColumnToColumn { src, stack_start: 0, dst })
            }
        }
        "cf" => {
            if tokens.len() < 3 {
                return Err("Usage: cf <src_col> <cell_idx>".to_string());
            }
            Ok(Command::ColumnToFreeCell {
                src_col: parse_col_idx(tokens[1])?,
                dst_cell: parse_cell_idx(tokens[2])?,
            })
        }
        "fc" => {
            if tokens.len() < 3 {
                return Err("Usage: fc <cell_idx> <dst_col>".to_string());
            }
            Ok(Command::FreeCellToColumn {
                src_cell: parse_cell_idx(tokens[1])?,
                dst_col: parse_col_idx(tokens[2])?,
            })
        }
        "ctf" => {
            if tokens.len() < 2 {
                return Err("Usage: ctf <src_col>".to_string());
            }
            Ok(Command::ColumnToFoundation { src: parse_col_idx(tokens[1])? })
        }
        "ftf" => {
            if tokens.len() < 2 {
                return Err("Usage: ftf <cell_idx>".to_string());
            }
            Ok(Command::FreeCellToFoundation { src_cell: parse_cell_idx(tokens[1])? })
        }
        "dragon" | "dr" => {
            if tokens.len() < 2 {
                return Err("Usage: dragon r|g|b".to_string());
            }
            let suit = parse_suit(tokens[1])?;
            Ok(Command::MergeDragons { suit })
        }

        "undo" | "u" => Ok(Command::Undo),
        "new" | "n" => Ok(Command::NewGame),
        "quit" | "q" | "exit" => Ok(Command::Quit),
        "help" | "h" | "?" => Ok(Command::Help),
        _ => Err(format!("Unknown command '{}'. Type 'help' for help.", tokens[0])),
    }
}

fn parse_col_idx(s: &str) -> Result<usize, String> {
    let n: usize = s
        .parse()
        .map_err(|_| format!("'{}' is not a valid column index", s))?;
    if n >= crate::board::NUM_COLUMNS {
        return Err(format!(
            "Column index {} out of range (0–{})",
            n,
            crate::board::NUM_COLUMNS - 1
        ));
    }
    Ok(n)
}

fn parse_cell_idx(s: &str) -> Result<usize, String> {
    let n: usize = s
        .parse()
        .map_err(|_| format!("'{}' is not a valid free-cell index", s))?;
    if n >= crate::board::NUM_FREE_CELLS {
        return Err(format!(
            "Free-cell index {} out of range (0–{})",
            n,
            crate::board::NUM_FREE_CELLS - 1
        ));
    }
    Ok(n)
}

fn parse_suit(s: &str) -> Result<crate::card::Suit, String> {
    match s.to_lowercase().as_str() {
        "r" | "red" => Ok(crate::card::Suit::Red),
        "g" | "green" => Ok(crate::card::Suit::Green),
        "b" | "black" => Ok(crate::card::Suit::Black),
        _ => Err(format!("'{}' is not a valid suit. Use r, g, or b.", s)),
    }
}

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
use std::collections::HashSet;

use crate::board::{Board, Location, NUM_COLUMNS, NUM_FREE_CELLS};
use crate::card::Suit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SolverMove {
    /// `depth_from_top`: 0 = only the top card, 1 = top two cards, etc.
    /// This matches the game's command syntax: `cc src:depth dst`.
    ColToCol { src: usize, dst: usize, depth_from_top: usize },
    ColToFree { src: usize, dst: usize },
    FreeToCol { src: usize, dst: usize },
    ColToFound { src: usize },
    FreeToFound { src: usize },
    Merge { suit: Suit },
}

impl SolverMove {
    /// Format this move as the game CLI command string the player would type.
    pub fn to_command_str(self) -> String {
        match self {
            SolverMove::ColToCol { src, dst, depth_from_top: 0 } =>
                format!("cc {} {}", src, dst),
            SolverMove::ColToCol { src, dst, depth_from_top: d } =>
                format!("cc {}:{} {}", src, d, dst),
            SolverMove::ColToFree { src, dst } =>
                format!("cf {} {}", src, dst),
            SolverMove::FreeToCol { src, dst } =>
                format!("fc {} {}", src, dst),
            SolverMove::ColToFound { src } =>
                format!("ctf {}", src),
            SolverMove::FreeToFound { src } =>
                format!("ftf {}", src),
            SolverMove::Merge { suit } => {
                let s = match suit {
                    Suit::Red   => "r",
                    Suit::Green => "g",
                    Suit::Black => "b",
                };
                format!("dragon {}", s)
            }
        }
    }
}

impl Board {
    /// Return all valid and productive moves from the current state.
    pub fn valid_moves(&self) -> Vec<SolverMove> {
        let mut moves = Vec::new();

        // 1. Merge dragons (if we can, we typically should!)
        for &suit in &Suit::ALL {
            if self.can_merge_dragons(suit) {
                // In many cases, if a merge is available, it's strictly optimal.
                // We'll add it as a move. Future optimization: if merge is possible, ONLY return merge.
                moves.push(SolverMove::Merge { suit });
            }
        }

        // 2. Column to Foundation
        for src_col in 0..NUM_COLUMNS {
            if !self.columns[src_col].is_empty() && self.can_move_to_foundation(Location::Column(src_col)) {
                moves.push(SolverMove::ColToFound { src: src_col });
            }
        }

        // 3. Free to Foundation
        for src_cell in 0..NUM_FREE_CELLS {
            if self.free_cell_card(src_cell).is_some() && self.can_move_to_foundation(Location::FreeCell(src_cell)) {
                moves.push(SolverMove::FreeToFound { src: src_cell });
            }
        }

        // 4. Column to Free Cell
        // Optimization: pick only the FIRST empty free cell. Identical otherwise.
        let first_empty = (0..NUM_FREE_CELLS).find(|&i| self.free_cells[i].is_empty());
        if let Some(dst_cell) = first_empty {
            for src_col in 0..NUM_COLUMNS {
                if !self.columns[src_col].is_empty() {
                    // Always valid to put single top card into an empty free cell
                    moves.push(SolverMove::ColToFree { src: src_col, dst: dst_cell });
                }
            }
        }

        // 5. Column to Column
        for src_col in 0..NUM_COLUMNS {
            let col_len = self.columns[src_col].len();
            if col_len == 0 { continue; }
            
            for start_idx in 0..col_len {
                // Check if [start_idx..col_len] is a valid movable stack
                if self.stack_len(src_col, start_idx) == col_len - start_idx {
                    let bottom_card = self.columns[src_col][start_idx];
                    // Convert absolute index → depth from top (0 = only top card)
                    let depth_from_top = col_len - 1 - start_idx;

                    for dst_col in 0..NUM_COLUMNS {
                        if src_col == dst_col { continue; }

                        let can_place = match self.column_top(dst_col) {
                            None => true,
                            Some(top) => bottom_card.can_stack_on(top),
                        };

                        if can_place {
                            // Skip moving an entire column to an empty column (symmetrical no-op)
                            if start_idx == 0 && self.column_top(dst_col).is_none() {
                                continue;
                            }
                            moves.push(SolverMove::ColToCol { src: src_col, dst: dst_col, depth_from_top });
                        }
                    }
                }
            }
        }

        // 6. Free to Column
        for src_cell in 0..NUM_FREE_CELLS {
            if let Some(card) = self.free_cell_card(src_cell) {
                for dst_col in 0..NUM_COLUMNS {
                    let can_place = match self.column_top(dst_col) {
                        None => true,
                        Some(top) => card.can_stack_on(top),
                    };
                    if can_place {
                        moves.push(SolverMove::FreeToCol { src: src_cell, dst: dst_col });
                    }
                }
            }
        }

        moves
    }

    /// Execute a solver move on this board.
    pub fn apply_move(&mut self, m: SolverMove) {
        match m {
            SolverMove::ColToCol { src, dst, depth_from_top } => {
                // Convert depth-from-top back to absolute index for move_stack
                let col_len = self.columns[src].len();
                let abs_idx = col_len - 1 - depth_from_top;
                self.move_stack(src, abs_idx, dst).unwrap();
            }
            SolverMove::ColToFree { src, dst } => { self.move_card(Location::Column(src), Location::FreeCell(dst)).unwrap(); }
            SolverMove::FreeToCol { src, dst } => { self.move_card(Location::FreeCell(src), Location::Column(dst)).unwrap(); }
            SolverMove::ColToFound { src } => { self.move_to_foundation(Location::Column(src)).unwrap(); }
            SolverMove::FreeToFound { src } => { self.move_to_foundation(Location::FreeCell(src)).unwrap(); }
            SolverMove::Merge { suit } => { self.merge_dragons(suit).unwrap(); }
        }
        // Always trigger safe auto-moves after any manual legal move
        let _ = self.auto_move();

    }
}

// ---------------------------------------------------------------------------
// Heuristic evaluation
// ---------------------------------------------------------------------------

/// Estimate how "close to winning" a board is.
/// Higher score = better position.
///
/// This is the `h(n)` component of A*.
fn heuristic(board: &Board) -> i32 {
    let mut score = 0i32;

    // +50 per card safely in the foundation (max 27 numbered + flower = 28 ultimate)
    for &f in &board.foundations {
        score += f as i32 * 50;
    }
    if board.flower_placed {
        score += 50;
    }

    // +80 per fully empty column (empty columns are very powerful – can park stacks)
    for col in &board.columns {
        if col.is_empty() {
            score += 80;
        }
    }

    // +25 per empty free cell
    for fc in &board.free_cells {
        if fc.is_empty() {
            score += 25;
        }
    }

    // Penalty: for each needed-but-buried card, count how many cards are above it.
    // "Needed" = the next card to go to the foundation for each suit.
    // The deeper it's buried, the harder the position.
    use crate::board::{FreeCellState, NUM_FOUNDATIONS};
    use crate::card::Card;
    let suits = crate::card::Suit::ALL;
    for (idx, &suit) in suits.iter().enumerate() {
        if idx >= NUM_FOUNDATIONS { break; }
        let needed_val = board.foundations[idx] + 1;
        if needed_val > 9 { continue; }
        let target = Card::Numbered(suit, needed_val);

        // Search every column for the target card and count how many cards are above it.
        'col_search: for col in &board.columns {
            for (depth_from_bottom, card) in col.iter().enumerate() {
                if *card == target {
                    let buried_depth = col.len() - 1 - depth_from_bottom; // 0 = on top
                    score -= buried_depth as i32 * 10;
                    break 'col_search;
                }
            }
        }
        // Check free cells
        for fc in &board.free_cells {
            if *fc == FreeCellState::Card(target) {
                // It's accessible immediately – small bonus
                score += 5;
            }
        }
    }

    score
}

// ---------------------------------------------------------------------------
// A* Search Node
// ---------------------------------------------------------------------------

use std::cmp::Ordering;
use std::collections::BinaryHeap;

struct SearchNode {
    /// f = g + h  (we want to minimise f, but Rust's BinaryHeap is a max-heap,
    /// so we store the **negated** f value so that the "best" node has the
    /// highest stored value and gets popped first.)
    neg_f: i32,
    /// Number of moves made so far (g cost).
    g: u32,
    board: Board,
    path: Vec<SolverMove>,
}

impl PartialEq for SearchNode {
    fn eq(&self, other: &Self) -> bool { self.neg_f == other.neg_f }
}
impl Eq for SearchNode {}

impl PartialOrd for SearchNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for SearchNode {
    fn cmp(&self, other: &Self) -> Ordering {
        // Primary key: neg_f (higher neg_f = lower real f = explored first)
        self.neg_f.cmp(&other.neg_f)
            // Secondary key: fewer steps taken first (prefer shorter paths when equal f)
            .then_with(|| other.g.cmp(&self.g))
    }
}

// ---------------------------------------------------------------------------
// A* solver
// ---------------------------------------------------------------------------

/// A* pathfinding solver.
///
/// Uses `heuristic()` to guide the search toward promising states first.
/// The `visited` HashSet prevents re-exploring the same board position.
/// Gives up after `NODE_LIMIT` nodes to keep the game responsive.
pub fn solve(initial_board: &Board) -> Option<Vec<SolverMove>> {
    let mut heap: BinaryHeap<SearchNode> = BinaryHeap::new();
    let mut visited: HashSet<Board> = HashSet::new();

    let mut start = initial_board.clone();
    let _ = start.auto_move();


    let h0 = heuristic(&start);
    heap.push(SearchNode { neg_f: h0, g: 0, board: start.clone(), path: Vec::new() });
    visited.insert(start);

    let mut nodes_explored = 0usize;
    const NODE_LIMIT: usize = 500_000;

    while let Some(SearchNode { board: state, path, g, .. }) = heap.pop() {
        if state.is_won() {
            println!("\nSolver: Found solution in {} moves! Explored {} nodes.", path.len(), nodes_explored);
            return Some(path);
        }

        nodes_explored += 1;
        if nodes_explored > NODE_LIMIT {
            println!("\nSolver: Node limit ({}) reached. No solution found.", NODE_LIMIT);
            return None;
        }

        if nodes_explored % 10_000 == 0 {
            println!("  ... {} nodes explored so far", nodes_explored);
        }

        for m in state.valid_moves() {
            let mut next = state.clone();
            next.apply_move(m);

            if visited.insert(next.clone()) {
                let g_next = g + 1;
                let h = heuristic(&next);
                // We want to MAXIMISE the score towards the goal.
                // Priority = h (board quality) - g_next (cost so far).
                // Stored as neg_f in the max-heap so best states are popped first.
                let neg_f = h - g_next as i32;

                let mut next_path = path.clone();
                next_path.push(m);
                heap.push(SearchNode { neg_f, g: g_next, board: next, path: next_path });
            }
        }
    }

    println!("\nSolver: Search exhausted ({} nodes), no solution found.", nodes_explored);
    None
}

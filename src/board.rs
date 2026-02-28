use rand::seq::SliceRandom;
use rand::SeedableRng;

use crate::card::{Card, Suit, full_deck};

/// Number of tableau columns.
pub const NUM_COLUMNS: usize = 8;
/// Number of free-cell slots.
pub const NUM_FREE_CELLS: usize = 3;
/// Number of foundation slots (one per suit).
pub const NUM_FOUNDATIONS: usize = 3;

/// A free-cell slot can be:
/// - Empty
/// - Holding a single card temporarily
/// - Locked by a set of four dragons
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FreeCellState {
    Empty,
    Card(Card),
    DragonLocked(Suit),
}

impl FreeCellState {
    pub fn is_empty(&self) -> bool {
        matches!(self, FreeCellState::Empty)
    }

    pub fn card(&self) -> Option<Card> {
        match self {
            FreeCellState::Card(c) => Some(*c),
            _ => None,
        }
    }
}

/// Source location for a card during a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
    /// The top card of a tableau column (0-indexed).
    Column(usize),
    /// A free-cell slot (0-indexed).
    FreeCell(usize),
}

/// The game board – the single source of truth for all game state.
#[derive(Debug, Clone)]
pub struct Board {
    /// 8 tableau columns; index 0 is leftmost.
    pub columns: [Vec<Card>; NUM_COLUMNS],
    /// 3 free-cell slots.
    pub free_cells: [FreeCellState; NUM_FREE_CELLS],
    /// Foundation progress per suit: the highest numbered card placed (0 = empty).
    pub foundations: [u8; NUM_FOUNDATIONS],
    /// Whether the flower slot is occupied.
    pub flower_placed: bool,
}

/// Maps a `Suit` to its foundation/free-cell array index.
fn suit_index(suit: Suit) -> usize {
    match suit {
        Suit::Red => 0,
        Suit::Green => 1,
        Suit::Black => 2,
    }
}

impl Board {
    // -------------------------------------------------------------------------
    // Construction / Dealing
    // -------------------------------------------------------------------------

    /// Deal a fresh shuffled board using a random seed.
    pub fn deal_random() -> Self {
        let mut rng = rand::rngs::SmallRng::from_os_rng();
        let mut deck = full_deck();
        deck.shuffle(&mut rng);
        Self::deal_from_deck(deck)
    }

    /// Deal a board from a specific seed (useful for reproducible games).
    pub fn deal_seeded(seed: u64) -> Self {
        let mut rng = rand::rngs::SmallRng::seed_from_u64(seed);
        let mut deck = full_deck();
        deck.shuffle(&mut rng);
        Self::deal_from_deck(deck)
    }

    /// Deal a board from an already-ordered deck slice (for testing).
    pub fn deal_from_deck(deck: Vec<Card>) -> Self {
        assert_eq!(deck.len(), 40, "Need exactly 40 cards to deal");

        // Distribute 40 cards across 8 columns: 5 columns get 5 cards, 3 get 4.
        // (5×5 + 3×4 = 25+12 = 37 -- No, that's wrong. 8*5=40, deal 5 each)
        // Actually: 40 / 8 = 5 cards per column.
        let mut columns: [Vec<Card>; NUM_COLUMNS] = Default::default();
        for (i, card) in deck.into_iter().enumerate() {
            columns[i % NUM_COLUMNS].push(card);
        }

        Board {
            columns,
            free_cells: [
                FreeCellState::Empty,
                FreeCellState::Empty,
                FreeCellState::Empty,
            ],
            foundations: [0; NUM_FOUNDATIONS],
            flower_placed: false,
        }
    }

    // -------------------------------------------------------------------------
    // Accessors
    // -------------------------------------------------------------------------

    /// Returns the top card of a column, if any.
    pub fn column_top(&self, col: usize) -> Option<Card> {
        self.columns[col].last().copied()
    }

    /// Returns the card in a free cell, if any.
    pub fn free_cell_card(&self, slot: usize) -> Option<Card> {
        self.free_cells[slot].card()
    }

    /// Returns the next card value that must go to a foundation for a suit.
    #[allow(dead_code)]
    pub fn next_foundation_value(&self, suit: Suit) -> u8 {
        self.foundations[suit_index(suit)] + 1
    }

    /// The card that *lives* at a given `Location` (top of column or free cell).
    pub fn card_at(&self, loc: Location) -> Option<Card> {
        match loc {
            Location::Column(c) => self.column_top(c),
            Location::FreeCell(f) => self.free_cell_card(f),
        }
    }

    // -------------------------------------------------------------------------
    // Move Validation
    // -------------------------------------------------------------------------

    /// Can the top card of `src` be moved to `dst`?
    pub fn can_move(&self, src: Location, dst: Location) -> bool {
        let card = match self.card_at(src) {
            Some(c) => c,
            None => return false,
        };

        match dst {
            Location::FreeCell(f) => {
                // Free cell must be empty (not locked, not occupied)
                self.free_cells[f].is_empty()
            }
            Location::Column(c) => {
                if c == match src { Location::Column(sc) => sc, _ => usize::MAX } {
                    return false; // same column
                }
                match self.column_top(c) {
                    // Empty column: any card is accepted
                    None => true,
                    // Non-empty: card must stack according to the rules
                    Some(top) => card.can_stack_on(top),
                }
            }
        }
    }

    /// Can the top card of `src` be moved to the foundation?
    pub fn can_move_to_foundation(&self, src: Location) -> bool {
        match self.card_at(src) {
            Some(Card::Flower) => !self.flower_placed,
            Some(Card::Numbered(suit, v)) => {
                self.foundations[suit_index(suit)] + 1 == v
            }
            _ => false,
        }
    }

    // -------------------------------------------------------------------------
    // Move Execution
    // -------------------------------------------------------------------------

    /// Move the top card from `src` to `dst` in the tableau / free cells.
    /// Returns `Err(reason)` if the move is illegal.
    pub fn move_card(&mut self, src: Location, dst: Location) -> Result<(), &'static str> {
        if !self.can_move(src, dst) {
            return Err("Illegal move");
        }

        let card = self.take_card(src).unwrap();
        self.place_card(dst, card);
        Ok(())
    }

    /// Move the top card from `src` to the appropriate foundation / flower slot.
    pub fn move_to_foundation(&mut self, src: Location) -> Result<(), &'static str> {
        if !self.can_move_to_foundation(src) {
            return Err("Card cannot go to foundation yet");
        }

        let card = self.take_card(src).unwrap();
        match card {
            Card::Flower => {
                self.flower_placed = true;
            }
            Card::Numbered(suit, _) => {
                self.foundations[suit_index(suit)] += 1;
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    /// Check whether all four dragons of `suit` are exposed (top of column or
    /// in a free cell) and therefore the merge can be performed.
    pub fn can_merge_dragons(&self, suit: Suit) -> bool {
        // Need a free cell that is either Empty or holding a dragon of the
        // same suit (it will be freed during the merge) to receive the lock.
        let dragon = Card::Dragon(suit);
        let has_slot = self
            .free_cells
            .iter()
            .any(|fc| fc.is_empty() || *fc == FreeCellState::Card(dragon));
        if !has_slot {
            return false;
        }

        let count = self.count_exposed_dragons(suit);
        count == 4
            && self
                .free_cells
                .iter()
                .filter(|fc| **fc == FreeCellState::Card(dragon))
                .count()
                // (Already counted in count_exposed_dragons; just confirming)
                <= 4
    }

    /// Count how many dragons of `suit` are currently exposed (column tops or free cells).
    fn count_exposed_dragons(&self, suit: Suit) -> usize {
        let dragon = Card::Dragon(suit);
        let in_cols = self
            .columns
            .iter()
            .filter(|col| col.last() == Some(&dragon))
            .count();
        let in_cells = self
            .free_cells
            .iter()
            .filter(|fc| **fc == FreeCellState::Card(dragon))
            .count();
        in_cols + in_cells
    }

    /// Merge all four exposed dragons of `suit` into a single locked free cell.
    /// Returns `Err` if the merge is not currently possible.
    pub fn merge_dragons(&mut self, suit: Suit) -> Result<(), &'static str> {
        if !self.can_merge_dragons(suit) {
            return Err("Cannot merge dragons: not all four are exposed or no free cell");
        }

        let dragon = Card::Dragon(suit);

        // Remove dragons from columns (only top cards)
        for col in self.columns.iter_mut() {
            if col.last() == Some(&dragon) {
                col.pop();
            }
        }
        // Remove dragons from free cells
        for fc in self.free_cells.iter_mut() {
            if *fc == FreeCellState::Card(dragon) {
                *fc = FreeCellState::Empty;
            }
        }

        // Lock one free cell with the dragon marker
        let slot = self
            .free_cells
            .iter_mut()
            .find(|fc| fc.is_empty())
            .expect("We verified a free slot exists");
        *slot = FreeCellState::DragonLocked(suit);

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Auto-Move
    // -------------------------------------------------------------------------

    /// Automatically move the flower card and any numbered cards that are safely
    /// auto-playable to the foundation.  Returns the number of cards moved.
    ///
    /// A numbered card is "safe" to auto-move when every suit has at least
    /// `value - 1` in its foundation (so we'll never need that card to build
    /// on), matching the original game's heuristic.
    pub fn auto_move(&mut self) -> usize {
        let mut moved = 0;

        // Iterate until no more moves are possible in one pass.
        loop {
            let before = moved;

            // Check all column tops and free cells.
            let sources: Vec<Location> = (0..NUM_COLUMNS)
                .map(Location::Column)
                .chain((0..NUM_FREE_CELLS).map(Location::FreeCell))
                .collect();

            for src in sources {
                if self.can_move_to_foundation(src) && self.is_safe_to_auto(src) {
                    let _ = self.move_to_foundation(src);
                    moved += 1;
                }
            }

            if moved == before {
                break; // No progress – stop.
            }
        }

        moved
    }

    /// A card is safe to auto-move to foundation when it's the flower OR when
    /// its foundation value is ≤ min(all_foundations) + 1.  This prevents
    /// moving a card needed as a stepping-stone.
    fn is_safe_to_auto(&self, src: Location) -> bool {
        match self.card_at(src) {
            Some(Card::Flower) => true,
            Some(Card::Numbered(_suit, v)) => {
                let min_found = *self.foundations.iter().min().unwrap();
                // Safe if every other foundation is within 1 of this card's value
                v <= min_found + 1 || v == 1
            }
            _ => false,
        }
    }

    // -------------------------------------------------------------------------
    // Win Condition
    // -------------------------------------------------------------------------

    /// The game is won when:
    /// - All foundations are at 9.
    /// - The flower is placed.
    /// - All free cells are either Empty or DragonLocked.
    /// - All columns are empty.
    pub fn is_won(&self) -> bool {
        self.foundations.iter().all(|&f| f == 9)
            && self.flower_placed
            && self.columns.iter().all(|col| col.is_empty())
            && self
                .free_cells
                .iter()
                .all(|fc| !matches!(fc, FreeCellState::Card(_)))
    }

    // -------------------------------------------------------------------------
    // Stack Move (multi-card)
    // -------------------------------------------------------------------------

    /// Returns the length of the movable stack starting from position `from_idx`
    /// in column `col`.  A stack is movable if it forms a valid descending,
    /// alternating-suit sequence.
    pub fn stack_len(&self, col: usize, from_idx: usize) -> usize {
        let col_cards = &self.columns[col];
        if from_idx >= col_cards.len() {
            return 0;
        }
        let mut len = 1;
        let mut i = from_idx;
        while i + 1 < col_cards.len() {
            if col_cards[i + 1].can_stack_on(col_cards[i]) {
                len += 1;
                i += 1;
            } else {
                break;
            }
        }
        len
    }

    /// Move a stack of cards from column `src_col` starting at `start_idx`
    /// to column `dst_col`.  All cards from `start_idx` to the bottom of the
    /// column are moved.
    pub fn move_stack(
        &mut self,
        src_col: usize,
        start_idx: usize,
        dst_col: usize,
    ) -> Result<(), &'static str> {
        if src_col == dst_col {
            return Err("Source and destination columns are the same");
        }

        let col_len = self.columns[src_col].len();
        if start_idx >= col_len {
            return Err("start_idx out of bounds");
        }

        // Verify the stack is a valid sequence.
        let movable = self.stack_len(src_col, start_idx);
        let stack_size = col_len - start_idx;
        if movable < stack_size {
            return Err("That slice is not a valid sequence");
        }

        // Validate placement of the bottom card of the stack onto the dst column.
        let bottom_card = self.columns[src_col][start_idx];
        if !match self.column_top(dst_col) {
            None => true, // Empty column accepts anything
            Some(top) => bottom_card.can_stack_on(top),
        } {
            return Err("Stack cannot be placed on destination column");
        }

        // Execute the move.
        let stack: Vec<Card> = self.columns[src_col].drain(start_idx..).collect();
        self.columns[dst_col].extend(stack);
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Internal helpers
    // -------------------------------------------------------------------------

    fn take_card(&mut self, loc: Location) -> Option<Card> {
        match loc {
            Location::Column(c) => self.columns[c].pop(),
            Location::FreeCell(f) => {
                let card = self.free_cells[f].card();
                if card.is_some() {
                    self.free_cells[f] = FreeCellState::Empty;
                }
                card
            }
        }
    }

    fn place_card(&mut self, loc: Location, card: Card) {
        match loc {
            Location::Column(c) => self.columns[c].push(card),
            Location::FreeCell(f) => {
                self.free_cells[f] = FreeCellState::Card(card);
            }
        }
    }
}

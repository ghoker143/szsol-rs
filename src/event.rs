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
use crate::board::Location;

use crate::card::Card;

/// Describes one atomic board state change.
///
/// **Design intent**: `Board::move_*` and `Board::auto_move` already return
/// `Vec<GameEvent>` even though every method currently returns an empty `Vec`.
/// This is intentional: the interface is intentionally forward-compatible so
/// that a future animation layer can receive a stream of events without any
/// changes to the engine or `Renderer` trait.
///
/// When animation support is added, each board-mutation method will populate
/// and return the appropriate variants here instead of `vec![]`.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum GameEvent {
    /// A single card moved from one slot to another.
    CardMoved { card: Card, src: Location, dst: Location },
    /// A valid sequence of cards moved between columns.
    StackMoved { stack: Vec<Card>, src_col: usize, dst_col: usize },
    /// Four dragons of the same suit were merged and a free cell was locked.
    DragonsMerged { suit: crate::card::Suit, locked_cell: usize },
    /// The game has been won.
    Won,
    /// A new game was dealt with the given seed.
    Dealt { seed: u64 },
}

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
mod solver;
mod board;
mod card;
mod config;
mod command;
mod event;
mod game;
mod history;
mod renderer;
mod tui_renderer;

use game::Game;
use renderer::CliRenderer;
use tui_renderer::TuiRenderer;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli_mode  = args.contains(&"--cli".to_string());
    let seed: Option<u64> = args.iter()
        .find(|a| !a.starts_with('-'))
        .and_then(|s| s.parse().ok());

    if cli_mode {
        let mut game = Game::init(seed, CliRenderer::new());
        game.run();
    } else {
        // Detect glyph display width BEFORE entering alternate screen / raw mode.

        let renderer = TuiRenderer::new().expect("Failed to initialise terminal");
        let mut game = Game::init(seed, renderer);
        game.run_tui();
    }
}

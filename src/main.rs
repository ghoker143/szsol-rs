mod board;
mod card;
mod command;
mod game;
mod renderer;

use board::Board;
use game::Game;
use renderer::CliRenderer;

fn main() {
    println!(
        r#"
┌─────────────────────────────────────────┐
│   SHENZHEN I/O Solitaire (CLI Edition)  │
│   Type 'help' or '?' for commands.      │
└─────────────────────────────────────────┘
"#
    );

    // Parse optional seed from command-line arguments for reproducible games.
    let seed: Option<u64> = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok());

    let board = match seed {
        Some(s) => {
            println!("Using seed: {}", s);
            Board::deal_seeded(s)
        }
        None => Board::deal_random(),
    };

    let renderer = CliRenderer::new();
    let mut game = Game::new(board, renderer);
    game.run();
}

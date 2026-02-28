mod board;
mod card;
mod command;
mod game;
mod history;
mod renderer;

use game::Game;
use renderer::CliRenderer;

fn main() {
    // Parse optional seed from command-line arguments for reproducible games.
    let seed: Option<u64> = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok());

    let renderer = CliRenderer::new();
    let mut game = Game::init(seed, renderer);
    game.run();
}

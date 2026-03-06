# 🀄 SZSOL-RS

A **SHENZHEN I/O Solitaire** clone written in Rust.

Inspired by [usrshare/szsol](https://github.com/usrshare/szsol).

## ✨ Features

- 🖥️ **TUI** — default mode, supports keyboard *and* mouse
- 🃏 Full solitaire gameplay matching the original SHENZHEN I/O rules
- ⏪ Undo support (up to 64 steps)
- 🎲 Reproducible games via optional seed (`szsol <seed>`)
- 🧠 **Built-in A\* Solver** — finds a winning line, or confirms a stuck board
- 💾 Auto-save & resume — picks up right where you left off
- 🏆 Persistent win counter with escalating job titles

## 🚀 Getting Started

### Build & Run

```bash
cargo build --release
cargo run                  # random game (TUI, default)
cargo run -- 42            # seeded game
cargo run -- --cli         # legacy CLI mode (text commands, good for SSH/scripts)
```

## 🎮 Controls (TUI)

### Keyboard

| Key | Action |
|-----|--------|
| `q w e r t y u i` | Select a tableau column |
| Same key again | Grow selection up the ordered stack |
| `1 2 3` | Select / target a free cell |
| Destination key | Move selected card(s) |
| `Enter` | Send selected card to foundation |
| `Esc` | Cancel selection |
| `D` → `r`/`g`/`b` | Merge dragons by suit |
| `Z` | Undo |
| `N` | New game |
| `?` | Toggle help overlay |
| `Ctrl-C` | Quit |

### Mouse

Click a column, free cell, or foundation to select and move cards. The help overlay (`?`) has the full reference.

## 🖨️ CLI Mode

The text-only mode (`--cli`) is preserved for 摸鱼 purposes.  
Type `help` inside the game to see all available commands, including `solve`.

## 🗺️ Roadmap

| Version | Milestone |
| ------- | --------- |
| **0.1.0** | ✅ Playable CLI game |
| **0.2.0** | 📂 Save files & win counter |
| **0.3.0** | 🧠 Solver |
| **0.7.0** | 🖥️ TUI |
| **1.0.0** | 🖥️ Animation in TUI |
| **1.5.0** | 🏎️ Multiplayer speed race *(maybe )* |

## 📜 License

This project is licensed under the **GNU General Public License v3.0 (GPLv3)**.

**Relicensing Notice:** As the sole developer and copyright holder of this project, I have officially transitioned the licensing from MIT to **GPLv3** effective [March 6, 2026]. 

I hereby re-license all previous contributions and the entire git history of this repository under the terms of GPLv3. Any further distribution, modification, or use of this codebase must comply with the GPLv3 requirements.

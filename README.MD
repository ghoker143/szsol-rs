# 🀄 SZSOL-RS

A CLI clone of the **SHENZHEN I/O Solitaire** card game, written in Rust.

Inspired by [usrshare/szsol](https://github.com/usrshare/szsol).

## ✨ Features

- 🃏 Full solitaire gameplay
- ⏪ Undo support (up to 64 steps)
- 🎲 Reproducible games via optional seed (`szsol <seed>`)
- 🧠 Embedded Solver: Built-in solver to suggest moves or prove if a board is actually winnable.

## 🚀 Getting Started

### Build & Run

```bash
cargo build --release
cargo run              # random game
cargo run -- 42        # reproducible game with seed 42
```

## 🗺️ Roadmap

| Version | Milestone |
| ------- | --------- |
| **0.1.0** | ✅ Playable CLI game |
| **0.2.0** | 📂 Save files & win counter |
| **0.3.0** | 🧠 Solver |
| **1.0.0** | 🖥️ TUI |
| **1.5.0** | 🏎️ Multiplayer speed race *(maybe )* |

## 📜 License

MIT
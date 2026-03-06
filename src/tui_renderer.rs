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
use std::collections::{HashMap, VecDeque};
use std::io::Stdout;

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame, Terminal,
};

use crate::board::{Board, FreeCellState, Location, NUM_COLUMNS, NUM_FREE_CELLS};
use crate::card::{Card, Suit};
use crate::event::GameEvent;
use crate::renderer::Renderer;

// ---------------------------------------------------------------------------
// Key bindings
// ---------------------------------------------------------------------------

pub const COL_KEYS: [char; 8] = ['q', 'w', 'e', 'r', 't', 'y', 'u', 'i'];
pub const FC_KEYS: [char; 3] = ['1', '2', '3'];

#[allow(dead_code)]
pub fn key_to_location(c: char) -> Option<Location> {
    if let Some(col) = COL_KEYS.iter().position(|&k| k == c) {
        return Some(Location::Column(col));
    }
    if let Some(fc) = FC_KEYS.iter().position(|&k| k == c) {
        return Some(Location::FreeCell(fc));
    }
    None
}

// ---------------------------------------------------------------------------
// CardSpec – runtime-detected layout constants
// ---------------------------------------------------------------------------

/// All card geometry is derived from this spec, which is created once at
/// startup after calling `term_detection::detect_glyph_cols()`.
#[derive(Clone, Copy, Debug)]
pub struct CardSpec {
    /// Display-column width of one suit glyph in this terminal.
    /// 1 for Western, 2 for CJK.  Detected at runtime.
    #[allow(dead_code)]
    pub glyph_cols: u16,
}

impl CardSpec {
    pub fn new(glyph_cols: u16) -> Self {
        Self { glyph_cols }
    }

    /// Display-column width of a single suit glyph in this terminal.
    pub fn glyph_display_w(self, suit: Suit) -> usize {
        let _ = (self, suit);
        1
    }

    /// Total display-column width of a card widget.
    pub fn card_w(self) -> u16 {
        let _ = self;
        9
    }

    /// Total row height of a full card widget.
    pub fn card_h(self) -> u16 { 5 }

    /// Inner display-column width (card_w minus the two border columns).
    pub fn inner_w(self) -> usize {
        (self.card_w() - 2) as usize
    }

    /// The glyph string for a given suit.
    pub fn suit_str(self, suit: Suit) -> &'static str {
        let _ = self;
        match suit {
            Suit::Red   => "♦",
            Suit::Green => "♣",
            Suit::Black => "♠",
        }
    }

    /// Flower glyph (always narrow; one-of-a-kind on the board).
    pub fn flower_str(self) -> &'static str { "✿" }
}

#[derive(Clone, Debug)]
struct CardFace {
    rank: String,
    rank_w: usize,
    suit: &'static str,
    suit_w: usize,
    center: String,
    center_w: usize,
    fg: Color,
}

impl CardFace {
    fn from_card(card: Card, spec: CardSpec) -> Self {
        match card {
            Card::Numbered(suit, value) => {
                let suit_str = spec.suit_str(suit);
                let suit_w = spec.glyph_display_w(suit);
                Self {
                    rank: value.to_string(),
                    rank_w: value.to_string().len(),
                    suit: suit_str,
                    suit_w,
                    center: String::new(),
                    center_w: 0,
                    fg: suit_color(suit),
                }
            }
            Card::Dragon(suit) => {
                let suit_str = spec.suit_str(suit);
                let suit_w = spec.glyph_display_w(suit);
                Self {
                    rank: "D".to_string(),
                    rank_w: 1,
                    suit: suit_str,
                    suit_w,
                    center: "DRG".to_string(),
                    center_w: 3,
                    fg: suit_color(suit),
                }
            }
            Card::Flower => Self {
                rank: "F".to_string(),
                rank_w: 1,
                suit: spec.flower_str(),
                suit_w: 1,
                center: String::new(),
                center_w: 0,
                fg: Color::Magenta,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Selection State
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionState {
    Idle,
    Column { col: usize, depth: usize },
    FreeCell { idx: usize },
    WaitDragonSuit,
}

// ---------------------------------------------------------------------------
// BoardLayout – maps Location → screen Rect for animation / mouse hit-test
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct BoardLayout {
    pub slots: HashMap<Location, Rect>,
}

impl BoardLayout {
    pub fn hit_test(&self, x: u16, y: u16) -> Option<Location> {
        for (loc, rect) in &self.slots {
            if x >= rect.x && x < rect.x + rect.width
                && y >= rect.y && y < rect.y + rect.height
            {
                return Some(*loc);
            }
        }
        None
    }

    #[allow(dead_code)]
    pub fn center_of(&self, loc: Location) -> Option<(u16, u16)> {
        self.slots.get(&loc).map(|r| (r.x + r.width / 2, r.y + r.height / 2))
    }
}

// ---------------------------------------------------------------------------
// Card rendering
// ---------------------------------------------------------------------------

fn suit_color(suit: Suit) -> Color {
    match suit {
        Suit::Red   => Color::Red,
        Suit::Green => Color::Green,
        Suit::Black => Color::Gray,
    }
}

fn padded_row(
    inner: usize,
    left_pad: usize,
    content: Span<'static>,
    content_w: usize,
    right_pad: usize,
    border_style: Style,
    border_v: &'static str,
) -> Line<'static> {
    debug_assert_eq!(left_pad + content_w + right_pad, inner);
    Line::from(vec![
        Span::styled(border_v, border_style),
        Span::raw(" ".repeat(left_pad)),
        content,
        Span::raw(" ".repeat(right_pad)),
        Span::styled(border_v, border_style),
    ])
}

/// Returns the number of Unicode scalar values in a string.
/// Note: this is NOT the terminal display width — wide (CJK) glyphs
/// each count as 1 here but render as 2 columns. Only call this for
/// labels that are known to be ASCII.
fn char_count(text: &str) -> usize {
    text.chars().count()
}

pub const CARD_PEEK_ROWS: usize = 2;

/// Render a full CARD_H-row card.
///
/// Layout (5 rows):
/// ```text
/// ╭───────╮
/// │4 ♦    │
/// │  DRG  │
/// │    ♦ 4│
/// ╰───────╯
/// ```
fn card_lines(card: Card, selected: bool, spec: CardSpec) -> Vec<Line<'static>> {
    let inner = spec.inner_w();

    let bstyle = if selected {
        Style::default().fg(Color::Blue)
    } else {
        Style::default().fg(Color::White)
    };
    let (tl, tr, bl, br, h, v) = ("╭", "╮", "╰", "╯", "─", "│");

    let face = CardFace::from_card(card, spec);
    let cstyle = Style::default().fg(face.fg).add_modifier(Modifier::BOLD);

    // Borders – plain box chars, no glyph
    let top = Line::from(Span::styled(format!("{}{}{}", tl, h.repeat(inner), tr), bstyle));
    let bot = Line::from(Span::styled(format!("{}{}{}", bl, h.repeat(inner), br), bstyle));

    if matches!(card, Card::Dragon(_)) {
        let center_pad = (inner - face.center_w) / 2;
        let empty = padded_row(inner, 0, Span::raw(String::new()), 0, inner, bstyle, v);
        let center = padded_row(
            inner,
            center_pad,
            Span::styled(face.center, cstyle),
            face.center_w,
            inner - face.center_w - center_pad,
            bstyle,
            v,
        );
        return vec![top, empty.clone(), center, empty, bot];
    }

    let top_label = format!("{} {}", face.rank, face.suit);
    let top_label_w = face.rank_w + 1 + face.suit_w;
    let top_row = padded_row(
        inner,
        0,
        Span::styled(top_label, cstyle),
        top_label_w,
        inner - top_label_w,
        bstyle,
        v,
    );
    let center_row = if face.center_w == 0 {
        padded_row(
            inner,
            0,
            Span::raw(String::new()),
            0,
            inner,
            bstyle,
            v,
        )
    } else {
        let center_pad = (inner - face.center_w) / 2;
        padded_row(
            inner,
            center_pad,
            Span::styled(face.center.clone(), cstyle),
            face.center_w,
            inner - face.center_w - center_pad,
            bstyle,
            v,
        )
    };
    let bottom_label = format!("{} {}", face.suit, face.rank);
    let bottom_label_w = face.suit_w + 1 + face.rank_w;
    let bottom_row = padded_row(
        inner,
        inner - bottom_label_w,
        Span::styled(bottom_label, cstyle),
        bottom_label_w,
        0,
        bstyle,
        v,
    );

    vec![top, top_row, center_row, bottom_row, bot]
}

/// Top visible rows for every covered tableau card.
fn card_peek_lines(card: Card, selected: bool, spec: CardSpec) -> Vec<Line<'static>> {
    let mut lines: Vec<_> = if let Card::Dragon(suit) = card {
        let inner = spec.inner_w();
        let border = Style::default().fg(if selected { Color::Blue } else { Color::White });
        let cstyle = Style::default().fg(suit_color(suit)).add_modifier(Modifier::BOLD);
        let top = Line::from(Span::styled(format!("╭{}╮", "─".repeat(inner)), border));
        let label = format!("D {}", spec.suit_str(suit));
        let label_w = char_count(&label);
        let row = padded_row(
            inner,
            0,
            Span::styled(label, cstyle),
            label_w,
            inner - label_w,
            border,
            "│",
        );
        vec![top, row]
    } else {
        card_lines(card, false, spec)
            .into_iter()
            .take(CARD_PEEK_ROWS)
            .collect()
    };

    if selected {
        let border = Style::default().fg(Color::Blue);
        for line in &mut lines {
            if let Some(first) = line.spans.first_mut() {
                *first = first.clone().style(border);
            }
            if let Some(last) = line.spans.last_mut() {
                *last = last.clone().style(border);
            }
        }
    }

    lines
}

/// Empty-slot placeholder rendered with the same proportions as a full card.
fn empty_slot(spec: CardSpec, label: Option<&str>) -> Vec<Line<'static>> {
    let inner = spec.inner_w();
    let dim   = Style::default().fg(Color::DarkGray);
    let text = label.unwrap_or("");
    let label_w = char_count(text);
    let left = inner.saturating_sub(label_w) / 2;
    let right = inner.saturating_sub(label_w + left);
    vec![
        Line::from(Span::styled(format!("╭{}╮", "─".repeat(inner)), dim)),
        Line::from(vec![
            Span::styled("│", dim),
            Span::raw(" ".repeat(left)),
            Span::styled(text.to_string(), dim),
            Span::raw(" ".repeat(right)),
            Span::styled("│", dim),
        ]),
        Line::from(Span::styled(format!("│{}│", " ".repeat(inner)), dim)),
        Line::from(Span::styled(format!("│{}│", " ".repeat(inner)), dim)),
        Line::from(Span::styled(format!("╰{}╯", "─".repeat(inner)), dim)),
    ]
}

// ---------------------------------------------------------------------------
// TuiRenderer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum LogLevel { Info, Error }

pub struct TuiRenderer {
    terminal:    Terminal<CrosstermBackend<Stdout>>,
    pub selection: SelectionState,
    layout:      BoardLayout,
    status_log:  VecDeque<(LogLevel, String)>,
    header_wins: usize,
    header_seed: u64,
    show_help:   bool,
    spec:        CardSpec,
}

impl TuiRenderer {
    const LOG_CAP: usize = 3;

    pub fn new() -> std::io::Result<Self> {
        let spec = CardSpec::new(1);
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        Ok(Self {
            terminal,
            selection: SelectionState::Idle,
            layout: BoardLayout::default(),
            status_log: VecDeque::with_capacity(Self::LOG_CAP),
            header_wins: 0,
            header_seed: 0,
            show_help: false,
            spec,
        })
    }

    fn push_log(&mut self, level: LogLevel, msg: String) {
        if self.status_log.len() >= Self::LOG_CAP { self.status_log.pop_front(); }
        self.status_log.push_back((level, msg));
    }

    fn clear_log(&mut self) {
        self.status_log.clear();
    }

    pub fn draw_board(&mut self, board: &Board) {
        let wins      = self.header_wins;
        let seed      = self.header_seed;
        let log: Vec<_> = self.status_log.iter().cloned().collect();
        let sel       = self.selection.clone();
        let show_help = self.show_help;
        let board     = board.clone();
        let spec      = self.spec;

        let mut new_layout = BoardLayout::default();

        let _ = self.terminal.draw(|frame| {
            let area = frame.area();
            let top_row_h = spec.card_h() + 1; // cards + key-label row

            let root = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),            // header bar
                    Constraint::Length(top_row_h),    // free cells + flower + foundations
                    Constraint::Min(spec.card_h() + 2), // tableau (at least one full card)
                    Constraint::Length(3),            // status bar
                ])
                .split(area);

            render_header_bar(frame, root[0], wins, seed);
            render_top_row(frame, root[1], &board, &sel, &mut new_layout, spec);
            render_tableau(frame, root[2], &board, &sel, &mut new_layout, spec);
            render_statusbar(frame, root[3], &log, &sel);

            if show_help { render_help_overlay(frame, area); }
        });

        self.layout = new_layout;
    }
}

// ---------------------------------------------------------------------------
// Sub-renderers
// ---------------------------------------------------------------------------

fn render_header_bar(frame: &mut Frame, area: Rect, wins: usize, seed: u64) {
    let rank = match wins {
        0       => "来面试的",
        1..=9   => "带薪如厕生",
        10..=24 => "划水工程师",
        25..=49 => "工位地缚灵",
        50..=99 => "需求粉碎机",
        _       => "摸鱼仙人",
    };
    let text = format!(
        " SHENZHEN I/O  │  Seed: {:<20}  │  Wins: {:>4}  │  {}",
        seed, wins, rank
    );
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        area,
    );
}

fn render_top_row(
    frame: &mut Frame,
    area: Rect,
    board: &Board,
    sel: &SelectionState,
    layout: &mut BoardLayout,
    spec: CardSpec,
) {
    let cw = spec.card_w();
    let ch = spec.card_h();

    // Horizontal split: free cells | gap | flower | gap | foundations
    let fc_block_w  = NUM_FREE_CELLS as u16 * (cw + 1) + 1;
    let found_w     = Suit::ALL.len() as u16 * (cw + 1) + 1;
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(fc_block_w), // free cells
            Constraint::Length(2),          // gap
            Constraint::Length(cw + 2),     // flower
            Constraint::Length(2),          // gap
            Constraint::Length(found_w),    // foundations
            Constraint::Min(0),             // overflow ignored
        ])
        .split(area);

    // ── Free cells ──────────────────────────────────────────────────────────
    for (i, fc) in board.free_cells.iter().enumerate() {
        let sx = cols[0].x + 1 + i as u16 * (cw + 1);
        let sr = Rect { x: sx, y: area.y, width: cw, height: ch };
        let is_sel = matches!(sel, SelectionState::FreeCell { idx } if *idx == i);

        let lines: Vec<Line> = match fc {
            FreeCellState::Empty => {
                let key = FC_KEYS[i].to_string();
                empty_slot(spec, Some(key.as_str()))
            }
            FreeCellState::Card(c) => {
                card_lines(*c, is_sel, spec)
            }
            FreeCellState::DragonLocked(suit) => {
                let inner = spec.inner_w();
                let color = suit_color(*suit);
                let border = Style::default().fg(color).add_modifier(Modifier::BOLD);
                let text = "LOCK";
                let top_label = format!("D {}", spec.suit_str(*suit));
                let bottom_label = format!("{} D", spec.suit_str(*suit));
                let top_w = char_count(&top_label);
                let bottom_w = char_count(&bottom_label);
                let lock_left = inner.saturating_sub(text.len()) / 2;
                let lock_right = inner.saturating_sub(text.len() + lock_left);
                vec![
                    Line::from(Span::styled(format!("╔{}╗", "═".repeat(inner)), border)),
                    padded_row(inner, 0, Span::styled(top_label, border), top_w, inner - top_w, border, "║"),
                    padded_row(inner, lock_left, Span::styled(text, border), text.len(), lock_right, border, "║"),
                    padded_row(inner, inner - bottom_w, Span::styled(bottom_label, border), bottom_w, 0, border, "║"),
                    Line::from(Span::styled(format!("╚{}╝", "═".repeat(inner)), border)),
                ]
            }
        };

        if sr.y + ch <= area.y + area.height {
            frame.render_widget(Paragraph::new(lines), sr);
        }
        layout.slots.insert(Location::FreeCell(i), sr);

        // Key label below card
        let ky = area.y + ch;
        if ky < area.y + area.height {
            let kr = Rect { x: sx + cw / 2, y: ky, width: 1, height: 1 };
            frame.render_widget(
                Paragraph::new(FC_KEYS[i].to_string())
                    .style(Style::default().fg(Color::DarkGray)),
                kr,
            );
        }
    }

    // ── Flower ───────────────────────────────────────────────────────────────
    let fx = cols[2].x + 1;
    let fr = Rect { x: fx, y: area.y, width: cw, height: ch };
    let flower_lines: Vec<Line> = if board.flower_placed {
        card_lines(Card::Flower, false, spec)
    } else {
        empty_slot(spec, Some(spec.flower_str()))
    };
    frame.render_widget(Paragraph::new(flower_lines), fr);

    // ── Foundations ──────────────────────────────────────────────────────────
    for (i, &suit) in Suit::ALL.iter().enumerate() {
        let sx = cols[4].x + 1 + i as u16 * (cw + 1);
        let sr = Rect { x: sx, y: area.y, width: cw, height: ch };
        let v  = board.foundations[i];

        let lines: Vec<Line> = if v == 0 {
            empty_slot(spec, Some(spec.suit_str(suit)))
        } else {
            card_lines(Card::Numbered(suit, v), false, spec)
        };
        frame.render_widget(Paragraph::new(lines), sr);
        layout.slots.insert(Location::Foundation(suit), sr);
    }

    let ky = area.y + ch;
    if ky < area.y + area.height {
        let label = "Enter";
        let label_w = label.len() as u16;
        let found_center = cols[4].x + cols[4].width / 2;
        let label_x = found_center.saturating_sub(label_w / 2);
        let kr = Rect { x: label_x, y: ky, width: label_w, height: 1 };
        frame.render_widget(
            Paragraph::new(label)
                .style(Style::default().fg(Color::DarkGray)),
            kr,
        );
    }
}

fn render_tableau(
    frame: &mut Frame,
    area: Rect,
    board: &Board,
    sel: &SelectionState,
    layout: &mut BoardLayout,
    spec: CardSpec,
) {
    let cw  = spec.card_w();
    let ch  = spec.card_h();
    let col_step = cw + 2; // 1 gap each side

    // Key labels row
    for (i, &k) in COL_KEYS.iter().enumerate() {
        let kx = area.x + i as u16 * col_step + cw / 2;
        let kr = Rect { x: kx, y: area.y, width: 1, height: 1 };
        if kr.x < area.x + area.width {
            frame.render_widget(
                Paragraph::new(k.to_string())
                    .style(Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM)),
                kr,
            );
        }
    }

    let cards_y = area.y + 1;
    let bottom  = area.y + area.height;

    for col_idx in 0..NUM_COLUMNS {
        let col_x = area.x + col_idx as u16 * col_step;
        let col   = &board.columns[col_idx];

        let sel_depth = match sel {
            SelectionState::Column { col, depth } if *col == col_idx => *depth,
            _ => 0,
        };

        // Empty column placeholder
        if col.is_empty() {
            let r = Rect { x: col_x, y: cards_y, width: cw, height: ch };
            if r.y + ch <= bottom {
                frame.render_widget(Paragraph::new(empty_slot(spec, None)), r);
            }
            layout.slots.insert(
                Location::Column(col_idx),
                Rect { x: col_x, y: cards_y, width: cw, height: ch },
            );
            continue;
        }

        let n     = col.len();
        let mut y = cards_y;

        for (ci, &card) in col.iter().enumerate() {
            let is_top = ci == n - 1;
            let dist   = n - 1 - ci;   // 0 = top card
            let is_sel = sel_depth > 0 && dist < sel_depth;

            if !is_top {
                // Render the same top slice a full card would expose under overlap.
                if y + CARD_PEEK_ROWS as u16 <= bottom {
                    let r = Rect { x: col_x, y, width: cw, height: CARD_PEEK_ROWS as u16 };
                    frame.render_widget(Paragraph::new(card_peek_lines(card, is_sel, spec)), r);
                }
                y += CARD_PEEK_ROWS as u16;
            } else {
                // Full card (CARD_H rows)
                if y + ch <= bottom {
                    let r = Rect { x: col_x, y, width: cw, height: ch };
                    frame.render_widget(
                        Paragraph::new(card_lines(card, is_sel, spec)),
                        r,
                    );
                }
                // Register the whole column extent for hit-test
                layout.slots.insert(
                    Location::Column(col_idx),
                    Rect { x: col_x, y: cards_y, width: cw, height: y + ch - cards_y },
                );
            }
        }
    }
}

fn render_statusbar(
    frame: &mut Frame,
    area: Rect,
    log: &[(LogLevel, String)],
    sel: &SelectionState,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);

    let hint = match sel {
        SelectionState::Idle =>
            Span::styled(
                " cols: q w e r t y u i  |  free cells: 1 2 3  |  D=dragon  Z=undo  N=new  ?=help  Ctrl-C=quit",
                Style::default().fg(Color::DarkGray)),
        SelectionState::Column { col, depth } =>
            Span::styled(
                format!(" Selected col {} ×{}  |  same key → grow stack  |  dest key → move  |  Esc=cancel",
                    COL_KEYS[*col], depth),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        SelectionState::FreeCell { idx } =>
            Span::styled(
                format!(" Selected cell {}  |  col key → move  |  Esc=cancel", idx + 1),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        SelectionState::WaitDragonSuit =>
            Span::styled(
                " Dragon merge: press r / g / b for suit  |  Esc=cancel",
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
    };
    frame.render_widget(Paragraph::new(Line::from(hint)), chunks[0]);

    let log_lines: Vec<Line> = log.iter().map(|(lvl, msg)| {
        let (prefix, color) = match lvl {
            LogLevel::Info  => ("[INFO]", Color::Cyan),
            LogLevel::Error => ("[ERR ]", Color::Red),
        };
        Line::from(vec![
            Span::styled(format!(" {} ", prefix), Style::default().fg(color).add_modifier(Modifier::BOLD)),
            Span::raw(msg.clone()),
        ])
    }).collect();

    frame.render_widget(
        Paragraph::new(log_lines).block(
            Block::default().borders(Borders::TOP)
                .border_style(Style::default().fg(Color::DarkGray))
        ),
        chunks[1],
    );
}

fn render_help_overlay(frame: &mut Frame, area: Rect) {
    let w = 68u16.min(area.width);
    let h = 22u16.min(area.height);
    let popup = Rect {
        x: area.width.saturating_sub(w) / 2,
        y: area.height.saturating_sub(h) / 2,
        width: w, height: h,
    };
    frame.render_widget(Clear, popup);
    let lines = vec![
        Line::from(Span::styled(" TUI Help",
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED))),
        Line::from(""),
        Line::from("  Keyboard"),
        Line::from("  q w e r t y u i   select a tableau column"),
        Line::from("  same key again    grow selection up a valid ordered stack"),
        Line::from("  1 2 3             select / target a free cell"),
        Line::from("  destination key   move selected card(s)"),
        Line::from("  Enter             send selected single card to foundation"),
        Line::from("  Esc               cancel selection"),
        Line::from("  D then r / g / b  merge dragons by suit"),
        Line::from("  Z                 undo"),
        Line::from("  N                 new game"),
        Line::from("  ?                 toggle this help"),
        Line::from(""),
        Line::from("  Mouse"),
        Line::from("  click column      select from clicked card up to the top"),
        Line::from("  click free cell   select that card"),
        Line::from("  click destination move selection there"),
        Line::from("  click foundation  send selected single card to foundation"),
        Line::from("  double-click dragon  try merge that dragon suit"),
        Line::from(""),
        Line::from("  Ctrl-C            quit"),
        Line::from(""),
        Line::from(Span::styled("  Press ? to close",
            Style::default().fg(Color::DarkGray))),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default().borders(Borders::ALL).title(" Help ")
                .style(Style::default().fg(Color::White))
        ),
        popup,
    );
}

// ---------------------------------------------------------------------------
// Drop: restore terminal
// ---------------------------------------------------------------------------

impl Drop for TuiRenderer {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        let _ = self.terminal.show_cursor();
    }
}

// ---------------------------------------------------------------------------
// TuiRendererExt
// ---------------------------------------------------------------------------

pub trait TuiRendererExt {
    fn get_selection(&self) -> &SelectionState;
    fn set_selection(&mut self, s: SelectionState);
    fn toggle_help(&mut self);
    fn hit_test(&self, x: u16, y: u16) -> Option<Location>;
    fn slot_rect(&self, loc: Location) -> Option<Rect>;
    fn clear_status_log(&mut self);
}

impl TuiRendererExt for TuiRenderer {
    fn get_selection(&self) -> &SelectionState { &self.selection }
    fn set_selection(&mut self, s: SelectionState) { self.selection = s; }
    fn toggle_help(&mut self) { self.show_help = !self.show_help; }
    fn hit_test(&self, x: u16, y: u16) -> Option<Location> { self.layout.hit_test(x, y) }
    fn slot_rect(&self, loc: Location) -> Option<Rect> { self.layout.slots.get(&loc).copied() }
    fn clear_status_log(&mut self) { self.clear_log(); }
}

// ---------------------------------------------------------------------------
// Renderer trait impl
// ---------------------------------------------------------------------------

impl Renderer for TuiRenderer {
    fn render(&mut self, board: &Board) { self.draw_board(board); }
    fn info(&mut self, msg: &str)  { self.push_log(LogLevel::Info,  msg.to_string()); }
    fn error(&mut self, msg: &str) { self.push_log(LogLevel::Error, msg.to_string()); }
    fn help(&mut self)  { self.show_help = !self.show_help; }
    fn win(&mut self)   { self.push_log(LogLevel::Info, "YOU WIN!  Press N for another game.".to_string()); }
    fn render_header(&mut self, total_wins: usize, seed: u64) {
        self.header_wins = total_wins;
        self.header_seed = seed;
    }
    fn push_events(&mut self, _events: Vec<GameEvent>) { /* stub – future AnimationState */ }
    fn tick(&mut self) { /* stub – future anim.tick() */ }
}

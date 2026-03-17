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
use std::time::{Duration, Instant};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    symbols,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, Paragraph},
    Frame, Terminal,
};

use crate::board::{Board, FreeCellState, Location, NUM_COLUMNS, NUM_FREE_CELLS};
use crate::card::{Card, Suit};
use crate::event::GameEvent;
use crate::renderer::Renderer;
use crate::solver::{SolverMove, SolverProgress};

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
// Hint State
// ---------------------------------------------------------------------------

/// Tracks the current hint solution path.
#[derive(Debug, Clone)]
pub enum HintState {
    /// No hint active.
    Inactive,
    /// Hint active: `steps` is the full remaining path, `idx` is the current step.
    Active { steps: Vec<SolverMove>, idx: usize },
}

impl HintState {
    pub fn is_active(&self) -> bool {
        matches!(self, HintState::Active { .. })
    }

    /// Current step move, if any.
    pub fn current_move(&self) -> Option<SolverMove> {
        match self {
            HintState::Active { steps, idx } => steps.get(*idx).copied(),
            HintState::Inactive => None,
        }
    }

    /// Advance to the next step. Returns `true` if steps are exhausted.
    pub fn advance(&mut self) -> bool {
        if let HintState::Active { steps, idx } = self {
            *idx += 1;
            if *idx >= steps.len() {
                *self = HintState::Inactive;
                return true;
            }
        }
        false
    }
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
const MIN_TUI_WIDTH: u16 = 86;
const MIN_TUI_HEIGHT: u16 = 17;

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
fn card_lines(card: Card, selected: bool, hint: bool, spec: CardSpec) -> Vec<Line<'static>> {
    let inner = spec.inner_w();

    let bstyle = if selected {
        Style::default().fg(Color::Blue)
    } else if hint {
        Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
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
fn card_peek_lines(card: Card, selected: bool, hint: bool, spec: CardSpec) -> Vec<Line<'static>> {
    let mut lines: Vec<_> = if let Card::Dragon(suit) = card {
        let inner = spec.inner_w();
        let border = if selected {
            Style::default().fg(Color::Blue)
        } else if hint {
            Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
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
        card_lines(card, false, false, spec)
            .into_iter()
            .take(CARD_PEEK_ROWS)
            .collect()
    };

    if selected || hint {
        let border = if selected {
            Style::default().fg(Color::Blue)
        } else {
            Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
        };
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

#[derive(Debug, Clone)]
pub struct ActiveAnimation {
    pub event: GameEvent,
    pub start_time: Instant,
    pub duration: Duration,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimStyle {
    Linear,
    EaseOutQuad,
    EaseOutCubic,
    EaseInOutQuad,
}

impl AnimStyle {
    pub fn interpolate(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            AnimStyle::Linear => t,
            AnimStyle::EaseOutQuad => 1.0 - (1.0 - t) * (1.0 - t),
            AnimStyle::EaseOutCubic => 1.0 - (1.0 - t).powi(3),
            AnimStyle::EaseInOutQuad => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimSpeed {
    Off,
    Fast,
    Normal,
    Slow,
}

impl AnimSpeed {
    pub fn next(self) -> Self {
        match self {
            AnimSpeed::Off => AnimSpeed::Fast,
            AnimSpeed::Fast => AnimSpeed::Normal,
            AnimSpeed::Normal => AnimSpeed::Slow,
            AnimSpeed::Slow => AnimSpeed::Off,
        }
    }

    pub fn scale(self) -> f32 {
        match self {
            AnimSpeed::Off => 0.0,
            AnimSpeed::Fast => 0.5,
            AnimSpeed::Normal => 1.0,
            AnimSpeed::Slow => 2.0,
        }
    }
}

pub struct TuiRenderer {
    terminal:    Terminal<CrosstermBackend<Stdout>>,
    pub selection: SelectionState,
    layout:      BoardLayout,
    status_log:  VecDeque<(LogLevel, String)>,
    header_wins: usize,
    header_seed: u64,
    show_help:   bool,
    solving:     bool,
    solving_message: String,
    solving_progress: u16,
    solving_frame: usize,
    solver_failed_until: Option<Instant>,
    spec:        CardSpec,
    pub hint:    HintState,
    // Animation state
    anim_queue:  VecDeque<GameEvent>,
    current_anim: Option<ActiveAnimation>,
    anim_board:  Option<Board>,
    pub anim_speed: AnimSpeed,
    pub anim_style: AnimStyle,
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
            solving: false,
            solving_message: "少女祈祷中".to_string(),
            solving_progress: 0,
            solving_frame: 0,
            solver_failed_until: None,
            spec,
            hint: HintState::Inactive,
            anim_queue: VecDeque::new(),
            current_anim: None,
            anim_board: None,
            anim_speed: AnimSpeed::Normal,
            anim_style: AnimStyle::EaseOutQuad,
        })
    }

    fn push_log(&mut self, level: LogLevel, msg: String) {
        if self.status_log.len() >= Self::LOG_CAP { self.status_log.pop_front(); }
        self.status_log.push_back((level, msg));
    }

    fn clear_log(&mut self) {
        self.status_log.clear();
    }

    /// Compute src_location for the current hint step (None for Merge — multiple sources).
    fn hint_locs(&self) -> Option<(Location, Option<Location>)> {
        match self.hint.current_move()? {
            SolverMove::ColToCol { src, .. }   => Some((Location::Column(src), None)),
            SolverMove::ColToFree { src, dst } => Some((Location::Column(src), Some(Location::FreeCell(dst)))),
            SolverMove::FreeToCol { src, dst } => Some((Location::FreeCell(src), Some(Location::Column(dst)))),
            SolverMove::ColToFound { src }     => Some((Location::Column(src), None)),
            SolverMove::FreeToFound { src }    => Some((Location::FreeCell(src), None)),
            SolverMove::Merge { .. }           => None,  // highlight handled by hint_merge_suit
        }
    }

    /// Compute dst_location for arrow drawing.
    fn hint_dst_loc(&self) -> Option<Location> {
        match self.hint.current_move()? {
            SolverMove::ColToCol { dst, .. }       => Some(Location::Column(dst)),
            SolverMove::ColToFree { dst, .. }      => Some(Location::FreeCell(dst)),
            SolverMove::FreeToCol { dst, .. }      => Some(Location::Column(dst)),
            SolverMove::ColToFound { .. }          => {
                // Point to first foundation slot
                Some(Location::Foundation(Suit::ALL[0]))
            }
            SolverMove::FreeToFound { .. }         => Some(Location::Foundation(Suit::ALL[0])),
            SolverMove::Merge { suit }             => Some(Location::Foundation(suit)),
        }
    }

    pub fn draw_board(&mut self, real_board: &Board) {
        let is_animating = self.current_anim.is_some() || !self.anim_queue.is_empty();
        
        let board_to_draw = if is_animating {
            // Keep the cached anim_board (or fallback to real_board if missing)
            self.anim_board.as_ref().unwrap_or(real_board)
        } else {
            // Not animating: seamlessly update our cached visual board to the real one
            self.anim_board = Some(real_board.clone());
            real_board
        };

        let mut board = board_to_draw.clone();

        let (hide_src, hide_dragon, hide_stack, hide_all) = match &self.current_anim {
            Some(anim) => match &anim.event {
                GameEvent::CardMoved { src, .. } => (Some(*src), None, None, false),
                GameEvent::DragonsMerged { suit, .. } => (None, Some(*suit), None, false),
                GameEvent::StackMoved { stack, src_col, .. } => (None, None, Some((*src_col, stack.len())), false),
                GameEvent::Dealt { .. } | GameEvent::RestoreDealt { .. } => (None, None, None, true),
                _ => (None, None, None, false),
            },
            None => (None, None, None, false),
        };

        if hide_all {
            board.columns.iter_mut().for_each(|c| c.clear());
            board.free_cells.iter_mut().for_each(|f| *f = FreeCellState::Empty);
            board.foundations = [0; 3];
            board.flower_placed = false;
        }

        // Pre-modify the local `board` copy so the renderer naturally skips the
        // source cards (they will be drawn as flying overlays instead).
        if let Some(src) = hide_src {
            match src {
                Location::Column(c) => { board.columns[c].pop(); }
                Location::FreeCell(f) => { board.free_cells[f] = FreeCellState::Empty; }
                _ => {}
            }
        }
        if let Some(suit) = hide_dragon {
            let dragon = Card::Dragon(suit);
            for col in board.columns.iter_mut() {
                if col.last() == Some(&dragon) { col.pop(); }
            }
            for fc in board.free_cells.iter_mut() {
                if *fc == FreeCellState::Card(dragon) { *fc = FreeCellState::Empty; }
            }
        }
        if let Some((c, count)) = hide_stack {
            let len = board.columns[c].len();
            board.columns[c].truncate(len.saturating_sub(count));
        }

        let wins      = self.header_wins;
        let seed      = self.header_seed;
        let log: Vec<_> = self.status_log.iter().cloned().collect();
        let sel       = self.selection.clone();
        let show_help = self.show_help;
        let spec      = self.spec;
        let hint_active = self.hint.is_active();
        let hint_src  = self.hint_locs().map(|(s, _)| s);
        let hint_mv   = self.hint.current_move();
        let hint_dst  = self.hint_dst_loc();
        let board_for_arrow = board_to_draw.clone();
        let hint_col_depth: Option<(usize, usize)> = match self.hint.current_move() {
            Some(SolverMove::ColToCol { src, depth_from_top, .. }) => Some((src, depth_from_top + 1)),
            Some(SolverMove::ColToFree { src, .. })                => Some((src, 1)),
            Some(SolverMove::ColToFound { src })                   => Some((src, 1)),
            _                                                      => None,
        };
        let hint_merge_suit: Option<Suit> = match self.hint.current_move() {
            Some(SolverMove::Merge { suit }) => Some(suit),
            _                               => None,
        };
        let solving = self.solving;
        let solving_message = self.solving_message.clone();
        let solving_progress = self.solving_progress;
        let solving_frame = self.solving_frame;
        let solver_failed = self
            .solver_failed_until
            .map(|until| Instant::now() < until)
            .unwrap_or(false);
        let speed = self.anim_speed;

        let mut new_layout = BoardLayout::default();

        // Check if we have an active animation to overlay
        let current_anim_clone = self.current_anim.clone();
        let anim_style_clone = self.anim_style;

        let _ = self.terminal.draw(|frame| {
            let area = frame.area();
            if area.width < MIN_TUI_WIDTH || area.height < MIN_TUI_HEIGHT {
                render_too_small(frame, area, wins, seed);
                return;
            }
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
            render_top_row(frame, root[1], &board, &sel, hint_src, hint_merge_suit, &mut new_layout, spec);
            render_tableau(frame, root[2], &board, &sel, hint_src, hint_col_depth, hint_merge_suit, &mut new_layout, spec);
            render_statusbar(frame, root[3], &log, &sel, hint_active, speed);

            if show_help { render_help_overlay(frame, area); }
            if solving   { render_solving_overlay(frame, area, &solving_message, solving_progress, solving_frame); }
            if solver_failed { render_solver_failed_overlay(frame, area); }

            if let (Some(mv), Some(dst_loc)) = (hint_mv, hint_dst) {
                render_hint_arrow(frame, &new_layout, mv, &board_for_arrow, dst_loc, spec);
            }

            // Draw Animation Overlay Custom
            if let Some(anim) = &current_anim_clone {
                render_animation_overlay(frame, root[1], root[2], anim, &board_to_draw, spec, anim_style_clone);
            }
        });

        self.layout = new_layout;
    }
}

// ---------------------------------------------------------------------------
// Sub-renderers
// ---------------------------------------------------------------------------

fn render_too_small(frame: &mut Frame, area: Rect, wins: usize, seed: u64) {
    let lines = vec![
        Line::from(Span::styled(
            " Screen too small ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(format!(" Current size: {}x{}", area.width, area.height)),
        Line::from(format!(" Required size: at least {}x{}", MIN_TUI_WIDTH, MIN_TUI_HEIGHT)),
        Line::from(""),
        Line::from(" Resize the terminal to continue."),
        Line::from(" The game is still running and will redraw automatically."),
        Line::from(""),
        Line::from(format!(" Seed: {}   Wins: {}", seed, wins)),
    ];

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" szsol-rs ")
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        area,
    );
}

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
    hint_src: Option<Location>,
    hint_merge_suit: Option<Suit>,
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
        let is_sel  = matches!(sel, SelectionState::FreeCell { idx } if *idx == i);
        let is_hint = hint_src == Some(Location::FreeCell(i))
            || matches!((hint_merge_suit, fc), (Some(s), FreeCellState::Card(c)) if *c == Card::Dragon(s));
        let lines: Vec<Line> = match fc {
            FreeCellState::Empty => {
                let key = FC_KEYS[i].to_string();
                empty_slot(spec, Some(key.as_str()))
            }
            FreeCellState::Card(c) => {
                card_lines(*c, is_sel, is_hint, spec)
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
        card_lines(Card::Flower, false, false, spec)
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
            card_lines(Card::Numbered(suit, v), false, false, spec)
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
    hint_src: Option<Location>,
    hint_col_depth: Option<(usize, usize)>,  // (col_idx, num_cards_being_moved)
    hint_merge_suit: Option<Suit>,
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
        let is_hint_col = hint_src == Some(Location::Column(col_idx));
        let _is_hint_col = is_hint_col; // kept for possible future use

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

            // is_hint: highlight cards that are part of the hinted move
            // For a column source: all cards from (col_len - depth) upward are highlighted
            // For a dragon merge: every Dragon card of that suit is highlighted
            let is_hint_card = if let Some((hcol, hdepth)) = hint_col_depth {
                hcol == col_idx && dist < hdepth
            } else if let Some(suit) = hint_merge_suit {
                card == Card::Dragon(suit)
            } else {
                false
            };

            if !is_top {
                // Render the same top slice a full card would expose under overlap.
                if y + CARD_PEEK_ROWS as u16 <= bottom {
                    let r = Rect { x: col_x, y, width: cw, height: CARD_PEEK_ROWS as u16 };
                    frame.render_widget(Paragraph::new(card_peek_lines(card, is_sel, is_hint_card, spec)), r);
                }
                y += CARD_PEEK_ROWS as u16;
            } else {
                // Full card (CARD_H rows)
                if y + ch <= bottom {
                    let r = Rect { x: col_x, y, width: cw, height: ch };
                    frame.render_widget(
                        Paragraph::new(card_lines(card, is_sel, is_hint_card, spec)),
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

/// Draw an L-shaped hint arrow from the source card's rendered position to the destination slot.
///
/// Source point: center of the topmost moved card (bottom-most in the stack = first card grabbed),
/// computed from tile geometry rather than column-slot rect.
/// Destination point: center of the destination slot rect.
fn render_hint_arrow(
    frame: &mut Frame,
    layout: &BoardLayout,
    mv: SolverMove,
    board: &Board,
    dst_loc: Location,
    spec: CardSpec,
) {
    let arrow_style = Style::default()
        .fg(Color::LightYellow)
        .add_modifier(Modifier::BOLD);

    // ── Compute source point ────────────────────────────────────────────────
    // For column sources: start from the center of the first card being moved
    // (i.e. the card furthest down in the stack away from the top).
    let src_point: Option<(u16, u16)> = match mv {
        // Column source with potentially multiple cards
        SolverMove::ColToCol { src, depth_from_top, .. } => {
            let col_len = board.columns[src].len();
            if let Some(col_rect) = layout.slots.get(&Location::Column(src)) {
                let first_moved_idx = col_len.saturating_sub(depth_from_top + 1);
                let card_y  = col_rect.y + first_moved_idx as u16 * CARD_PEEK_ROWS as u16;
                let card_cx = col_rect.x + spec.card_w() / 2;
                // Top card renders as full height; non-top cards render as peek
                let card_cy = if depth_from_top == 0 {
                    card_y + spec.card_h() / 2       // full card center
                } else {
                    card_y + CARD_PEEK_ROWS as u16 / 2  // peek center
                };
                Some((card_cx, card_cy))
            } else { None }
        }
        // Column source, single top card
        SolverMove::ColToFree { src, .. } | SolverMove::ColToFound { src } => {
            let col_len = board.columns[src].len();
            if let Some(col_rect) = layout.slots.get(&Location::Column(src)) {
                let top_y   = col_rect.y + col_len.saturating_sub(1) as u16 * CARD_PEEK_ROWS as u16;
                let card_cx = col_rect.x + spec.card_w() / 2;
                Some((card_cx, top_y + spec.card_h() / 2))  // full card center
            } else { None }
        }
        SolverMove::FreeToCol { src, .. } | SolverMove::FreeToFound { src } => {
            layout.slots.get(&Location::FreeCell(src))
                .map(|r| (r.x + r.width / 2, r.y + r.height / 2))
        }
        SolverMove::Merge { .. } => None,
    };

    // ── Compute destination point ────────────────────────────────────────────
    // For column destinations: tip = top of where the moved card(s) will appear,
    // i.e. col_rect.y + current_col_len × CARD_PEEK_ROWS (the next peek slot).
    let dst_point: Option<(u16, u16)> = match mv {
        // Column destination: center of the current top card (the one being stacked on)
        SolverMove::ColToCol { dst, .. } | SolverMove::FreeToCol { dst, .. } => {
            let col_len = board.columns[dst].len();
            layout.slots.get(&Location::Column(dst)).map(|r| {
                let center_y = if col_len == 0 {
                    r.y + spec.card_h() / 2  // empty slot: center of placeholder
                } else {
                    r.y + (col_len - 1) as u16 * CARD_PEEK_ROWS as u16 + spec.card_h() / 2
                };
                (r.x + spec.card_w() / 2, center_y)
            })
        }
        SolverMove::ColToFree { dst, .. } => {
            layout.slots.get(&Location::FreeCell(dst))
                .map(|r| (r.x + r.width / 2, r.y + r.height / 2))
        }
        _ => {
            // Foundation / Merge: use slot center
            layout.slots.get(&dst_loc)
                .map(|r| (r.x + r.width / 2, r.y + r.height / 2))
        }
    };

    let (Some((src_cx, src_cy)), Some((dst_cx, dst_cy))) = (src_point, dst_point) else {
        return;
    };

    if src_cx == dst_cx && src_cy == dst_cy { return; }

    // Both source and destination anchored at horizontal center; dst y = landing top.

    if src_cy == dst_cy {
        // Pure horizontal arrow
        let going_right = src_cx < dst_cx;
        let (body_l, body_r) = if going_right {
            (src_cx + 1, dst_cx.saturating_sub(1))
        } else {
            (dst_cx + 1, src_cx.saturating_sub(1))
        };
        for x in body_l..=body_r {
            frame.render_widget(
                Paragraph::new("─").style(arrow_style),
                Rect { x, y: src_cy, width: 1, height: 1 },
            );
        }
        let tip = if going_right { "▶" } else { "◀" };
        frame.render_widget(
            Paragraph::new(tip).style(arrow_style),
            Rect { x: dst_cx, y: dst_cy, width: 1, height: 1 },
        );
    } else if src_cx == dst_cx {
        // Pure vertical arrow (same column)
        let going_down = src_cy < dst_cy;
        let (body_t, body_b) = if going_down {
            (src_cy + 1, dst_cy.saturating_sub(1))
        } else {
            (dst_cy + 1, src_cy.saturating_sub(1))
        };
        for y in body_t..=body_b {
            frame.render_widget(
                Paragraph::new("│").style(arrow_style),
                Rect { x: src_cx, y, width: 1, height: 1 },
            );
        }
        let tip = if going_down { "▼" } else { "▲" };
        frame.render_widget(
            Paragraph::new(tip).style(arrow_style),
            Rect { x: dst_cx, y: dst_cy, width: 1, height: 1 },
        );
    } else {
        // L-shaped: horizontal at src row, then vertical to dst row
        // The turn column is dst_cx.
        let going_right = src_cx <= dst_cx;
        let going_down  = src_cy < dst_cy;

        // Horizontal segment (from src_cx toward dst_cx, stopping before corner)
        if src_cx != dst_cx {
            let (lx, rx) = if going_right {
                (src_cx, dst_cx.saturating_sub(1))
            } else {
                (dst_cx + 1, src_cx)
            };
            for x in lx..=rx {
                frame.render_widget(
                    Paragraph::new("─").style(arrow_style),
                    Rect { x, y: src_cy, width: 1, height: 1 },
                );
            }
        }

        // Corner at (dst_cx, src_cy)
        // incoming horizontal from left + outgoing down   => ╮
        // incoming horizontal from left + outgoing up     => ╯
        // incoming horizontal from right + outgoing down  => ╭
        // incoming horizontal from right + outgoing up    => ╰
        let corner = match (going_right, going_down) {
            (true,  true)  => "╮",
            (true,  false) => "╯",
            (false, true)  => "╭",
            (false, false) => "╰",
        };
        frame.render_widget(
            Paragraph::new(corner).style(arrow_style),
            Rect { x: dst_cx, y: src_cy, width: 1, height: 1 },
        );

        // Vertical segment between corner and tip (exclusive)
        let (vy_start, vy_end) = if going_down {
            (src_cy + 1, dst_cy)
        } else {
            (dst_cy + 1, src_cy)
        };
        for y in vy_start..vy_end {
            frame.render_widget(
                Paragraph::new("│").style(arrow_style),
                Rect { x: dst_cx, y, width: 1, height: 1 },
            );
        }

        // Tip
        let tip = if going_down { "▼" } else { "▲" };
        frame.render_widget(
            Paragraph::new(tip).style(arrow_style),
            Rect { x: dst_cx, y: dst_cy, width: 1, height: 1 },
        );
    }
}

fn render_statusbar(
    frame: &mut Frame,
    area: Rect,
    log: &[(LogLevel, String)],
    sel: &SelectionState,
    hint_active: bool,
    anim_speed: AnimSpeed,
) {
    let speed_label = match anim_speed {
        AnimSpeed::Off => "Off",
        AnimSpeed::Fast => "Fast",
        AnimSpeed::Normal => "Norm",
        AnimSpeed::Slow => "Slow",
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);

    let hint = match sel {
        SelectionState::Idle if hint_active =>
            Span::styled(
                " HINT  |  Green = next card to move  |  H = exit  |  wrong move auto-exits",
                Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
        SelectionState::Idle =>
            Span::styled(
                format!(" cols: q w e r t y u i  |  cells: 1 2 3  |  D=drgn H=hint S=spd({}) Z=undo N=new Ctrl-C=exit", speed_label),
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

fn render_solving_overlay(frame: &mut Frame, area: Rect, _message: &str, progress: u16, frame_idx: usize) {
    const FRAMES: [&str; 8] = [
        "(>_<)  .",
        "(>_<)  *",
        "(-_-)  .",
        "(-_-)  *",
        "(u_u)  .",
        "(u_u)  *",
        "(-_-)  .",
        "(-_-)  *",
    ];

    let w = 30u16.min(area.width);
    let h = 5u16.min(area.height);
    let popup = Rect {
        x: area.width.saturating_sub(w) / 2,
        y: area.height.saturating_sub(h) / 2,
        width: w, height: h,
    };
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(symbols::border::ROUNDED)
        .border_style(Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD))
        .title(Span::styled(" Solver ", Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD)));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {} ", FRAMES[frame_idx % FRAMES.len()]),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "少女祈祷中",
                Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD),
            ),
        ])),
        chunks[0],
    );

    frame.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(Color::LightYellow).bg(Color::DarkGray).add_modifier(Modifier::BOLD))
            .label(format!("{}%", progress))
            .percent(progress),
        chunks[1],
    );
}

fn render_solver_failed_overlay(frame: &mut Frame, area: Rect) {
    let w = 28u16.min(area.width);
    let h = 5u16.min(area.height);
    let popup = Rect {
        x: area.width.saturating_sub(w) / 2,
        y: area.height.saturating_sub(h) / 2,
        width: w,
        height: h,
    };

    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(symbols::border::ROUNDED)
        .border_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        .title(Span::styled(" Solver ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No solution found  ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
        ]),
        inner,
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
        Line::from("  S                 toggle animation speed"),
        Line::from("  N                 new game"),
        Line::from("  H                 run solver hint / exit hint mode"),
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

fn get_slot_pos(loc: &Location, board: &Board, spec: CardSpec, top_area: Rect, tab_area: Rect, is_src: bool) -> (u16, u16) {
    let cw = spec.card_w();
    let col_step = cw + 2;
    match loc {
        Location::Column(c) => {
            let x = tab_area.x + *c as u16 * col_step;
            let mut cards_in_col = board.columns[*c].len() as u16;
            if is_src && cards_in_col > 0 {
                cards_in_col -= 1;
            }
            let y = tab_area.y + 1 + cards_in_col * CARD_PEEK_ROWS as u16;
            (x, y)
        }
        Location::FreeCell(fc) => {
            (top_area.x + 1 + *fc as u16 * (cw + 1), top_area.y)
        }
        Location::Flower => {
            let fc_block_w = NUM_FREE_CELLS as u16 * (cw + 1) + 1;
            (top_area.x + fc_block_w + 3, top_area.y)
        }
        Location::Foundation(suit) => {
            let idx = match suit { Suit::Red => 0, Suit::Green => 1, Suit::Black => 2 };
            let fc_block_w = NUM_FREE_CELLS as u16 * (cw + 1) + 1;
            let start = top_area.x + fc_block_w + cw + 6;
            (start + 1 + idx as u16 * (cw + 1), top_area.y)
        }
    }
}

fn render_animation_overlay(
    frame: &mut Frame,
    top_row_rect: Rect,
    tab_rect: Rect,
    anim: &ActiveAnimation,
    board: &Board,
    spec: CardSpec,
    style: AnimStyle,
) {
    let p = anim.start_time.elapsed().as_secs_f32() / anim.duration.as_secs_f32();
    let p = style.interpolate(p);

    let area_max_x = frame.area().width;
    let area_max_y = frame.area().height;

    match &anim.event {
        GameEvent::CardMoved { card, src, dst } => {
            let (sx, sy) = get_slot_pos(src, board, spec, top_row_rect, tab_rect, true);
            let (dx, dy) = get_slot_pos(dst, board, spec, top_row_rect, tab_rect, false);

            let cx = sx as f32 + (dx as f32 - sx as f32) * p;
            let cy = sy as f32 + (dy as f32 - sy as f32) * p;
            
            let x = cx.round() as u16;
            let y = cy.round() as u16;
            
            if x + spec.card_w() <= area_max_x && y + spec.card_h() <= area_max_y {
                let cr = Rect { x, y, width: spec.card_w(), height: spec.card_h() };
                frame.render_widget(Clear, cr);
                frame.render_widget(
                    Paragraph::new(card_lines(*card, false, false, spec))
                        .style(Style::default().bg(Color::Reset)),
                    cr
                );
            }
        }
        GameEvent::DragonsMerged { suit, locked_cell } => {
            let dest_loc = Location::FreeCell(*locked_cell);
            let (dx, dy) = get_slot_pos(&dest_loc, board, spec, top_row_rect, tab_rect, false);
            let dragon_card = Card::Dragon(*suit);

            let mut sources = Vec::new();
            for (i, fc) in board.free_cells.iter().enumerate() {
                if *fc == FreeCellState::Card(dragon_card) {
                    sources.push(Location::FreeCell(i));
                }
            }
            for (i, col) in board.columns.iter().enumerate() {
                if col.last() == Some(&dragon_card) {
                    sources.push(Location::Column(i));
                }
            }

            for src in sources {
                let (sx, sy) = get_slot_pos(&src, board, spec, top_row_rect, tab_rect, true);
                let cx = sx as f32 + (dx as f32 - sx as f32) * p;
                let cy = sy as f32 + (dy as f32 - sy as f32) * p;
                let x = cx.round() as u16;
                let y = cy.round() as u16;
                
                if x + spec.card_w() <= area_max_x && y + spec.card_h() <= area_max_y {
                    let cr = Rect { x, y, width: spec.card_w(), height: spec.card_h() };
                    frame.render_widget(Clear, cr);
                    frame.render_widget(
                        Paragraph::new(card_lines(dragon_card, false, false, spec))
                            .style(Style::default().bg(Color::Reset)),
                        cr
                    );
                }
            }
        }
        GameEvent::StackMoved { stack, src_col, dst_col } => {
            let cw = spec.card_w();
            let col_step = cw + 2;
            let cx_src = tab_rect.x + *src_col as u16 * col_step;
            let cx_dst = tab_rect.x + *dst_col as u16 * col_step;
            
            let full_src_len = board.columns[*src_col].len();
            let stack_base_idx = full_src_len.saturating_sub(stack.len());
            let src_y_base = tab_rect.y + 1 + stack_base_idx as u16 * CARD_PEEK_ROWS as u16;
            
            let dst_len = board.columns[*dst_col].len();
            let dst_y_base = tab_rect.y + 1 + dst_len as u16 * CARD_PEEK_ROWS as u16;
            
            let raw_t = anim.start_time.elapsed().as_secs_f32() / anim.duration.as_secs_f32();

            for (i, &card) in stack.iter().enumerate() {
                // Slinky trail effect: subordinate cards lag behind the top card initially
                // and snap into place at the end.
                let lag = 0.15 * i as f32;
                let local_t = (raw_t * (1.0 + lag) - lag).clamp(0.0, 1.0);
                let p_i = style.interpolate(local_t);

                let trail_x = cx_src as f32 + (cx_dst as f32 - cx_src as f32) * p_i;
                let trail_y = src_y_base as f32 + (dst_y_base as f32 - src_y_base as f32) * p_i;

                let x = trail_x.round() as u16;
                let y = (trail_y + i as f32 * CARD_PEEK_ROWS as f32).round() as u16;
                
                if x + spec.card_w() <= area_max_x && y + spec.card_h() <= area_max_y {
                    let cr = Rect { x, y, width: spec.card_w(), height: spec.card_h() };
                    frame.render_widget(Clear, cr);
                    frame.render_widget(
                        Paragraph::new(card_lines(card, false, false, spec))
                            .style(Style::default().bg(Color::Reset)),
                        cr
                    );
                }
            }
        }
        GameEvent::Dealt { seed } => {
            let new_board = Board::deal_seeded(*seed);
            
            // source position (bottom right corner of tab_rect)
            let sx = tab_rect.x + tab_rect.width.saturating_sub(spec.card_w());
            let sy = tab_rect.y + tab_rect.height.saturating_sub(spec.card_h());
            
            let raw_t = anim.start_time.elapsed().as_secs_f32() / anim.duration.as_secs_f32();
            let total_cards = 40;
            
            let mut i = 0;
            for (col_idx, col) in new_board.columns.iter().enumerate() {
                for (row_idx, &card) in col.iter().enumerate() {
                    let cw = spec.card_w();
                    let col_step = cw + 2;
                    let dx = tab_rect.x + col_idx as u16 * col_step;
                    let dy = tab_rect.y + 1 + row_idx as u16 * CARD_PEEK_ROWS as u16;

                    let start_t = (i as f32) / (total_cards as f32) * 0.5;
                    let duration_t = 0.5;

                    if raw_t > start_t {
                        let local_t = ((raw_t - start_t) / duration_t).clamp(0.0, 1.0);
                        let p_i = style.interpolate(local_t);

                        let cx = sx as f32 + (dx as f32 - sx as f32) * p_i;
                        let cy = sy as f32 + (dy as f32 - sy as f32) * p_i;

                        let x = cx.round() as u16;
                        let y = cy.round() as u16;

                        if x + spec.card_w() <= area_max_x && y + spec.card_h() <= area_max_y {
                            let cr = Rect { x, y, width: spec.card_w(), height: spec.card_h() };
                            frame.render_widget(Clear, cr);
                            frame.render_widget(
                                Paragraph::new(card_lines(card, false, false, spec))
                                    .style(Style::default().bg(Color::Reset)),
                                cr
                            );
                        }
                    }
                    i += 1;
                }
            }
        }
        GameEvent::RestoreDealt { board: restored_board } => {
            let new_board = restored_board;
            
            // source position (bottom right corner of tab_rect)
            let sx = tab_rect.x + tab_rect.width.saturating_sub(spec.card_w());
            let sy = tab_rect.y + tab_rect.height.saturating_sub(spec.card_h());
            
            let raw_t = anim.start_time.elapsed().as_secs_f32() / anim.duration.as_secs_f32();
            let total_cards = 40;
            
            let mut i = 0;
            for (col_idx, col) in new_board.columns.iter().enumerate() {
                for (row_idx, &card) in col.iter().enumerate() {
                    let cw = spec.card_w();
                    let col_step = cw + 2;
                    let dx = tab_rect.x + col_idx as u16 * col_step;
                    let dy = tab_rect.y + 1 + row_idx as u16 * CARD_PEEK_ROWS as u16;

                    let start_t = (i as f32) / (total_cards as f32) * 0.5;
                    let duration_t = 0.5;

                    if raw_t > start_t {
                        let local_t = ((raw_t - start_t) / duration_t).clamp(0.0, 1.0);
                        let p_i = style.interpolate(local_t);

                        let cx = sx as f32 + (dx as f32 - sx as f32) * p_i;
                        let cy = sy as f32 + (dy as f32 - sy as f32) * p_i;

                        let x = cx.round() as u16;
                        let y = cy.round() as u16;

                        if x + spec.card_w() <= area_max_x && y + spec.card_h() <= area_max_y {
                            let cr = Rect { x, y, width: spec.card_w(), height: spec.card_h() };
                            frame.render_widget(Clear, cr);
                            frame.render_widget(
                                Paragraph::new(card_lines(card, false, false, spec))
                                    .style(Style::default().bg(Color::Reset)),
                                cr
                            );
                        }
                    }
                    i += 1;
                }
            }
        }
        _ => {}
    }
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

    // Hint management
    fn set_hint_steps(&mut self, steps: Vec<SolverMove>);
    fn clear_hint(&mut self);
    fn advance_hint(&mut self) -> bool;   // true = hint exhausted
    fn hint_next_move(&self) -> Option<SolverMove>;
    fn is_hint_active(&self) -> bool;
    fn is_animating(&self) -> bool;
    fn toggle_anim_speed(&mut self);
    fn set_anim_speed(&mut self, speed: AnimSpeed);
    fn anim_speed(&self) -> AnimSpeed;
    fn sync_board(&mut self, board: &Board);
    // Solving overlay
    fn show_solving(&mut self);
    fn hide_solving(&mut self);
    fn update_solving_progress(&mut self, progress: SolverProgress);
    fn show_solver_failed(&mut self);
}

impl TuiRendererExt for TuiRenderer {
    fn get_selection(&self) -> &SelectionState { &self.selection }
    fn set_selection(&mut self, s: SelectionState) { self.selection = s; }
    fn toggle_help(&mut self) { self.show_help = !self.show_help; }
    fn hit_test(&self, x: u16, y: u16) -> Option<Location> { self.layout.hit_test(x, y) }
    fn slot_rect(&self, loc: Location) -> Option<Rect> { self.layout.slots.get(&loc).copied() }
    fn clear_status_log(&mut self) { self.clear_log(); }

    fn set_hint_steps(&mut self, steps: Vec<SolverMove>) {
        self.hint = HintState::Active { steps, idx: 0 };
    }
    fn clear_hint(&mut self) {
        self.hint = HintState::Inactive;
    }
    fn advance_hint(&mut self) -> bool {
        self.hint.advance()
    }
    fn hint_next_move(&self) -> Option<SolverMove> {
        self.hint.current_move()
    }
    fn is_hint_active(&self) -> bool {
        self.hint.is_active()
    }
    fn is_animating(&self) -> bool {
        self.current_anim.is_some() || !self.anim_queue.is_empty()
    }
    fn toggle_anim_speed(&mut self) {
        self.anim_speed = self.anim_speed.next();
    }
    fn set_anim_speed(&mut self, speed: AnimSpeed) {
        self.anim_speed = speed;
    }
    fn anim_speed(&self) -> AnimSpeed {
        self.anim_speed
    }
    fn sync_board(&mut self, board: &Board) {
        self.anim_queue.clear();
        self.current_anim = None;
        self.anim_board = Some(board.clone());
    }
    fn show_solving(&mut self) {
        self.solving = true;
        self.solving_message = "少女祈祷中".to_string();
        self.solving_progress = 0;
        self.solving_frame = 0;
    }
    fn hide_solving(&mut self) { self.solving = false; }
    fn update_solving_progress(&mut self, progress: SolverProgress) {
        self.solving_message = match progress {
            SolverProgress::Started { .. } => "正在整理牌堆与搜索空间".to_string(),
            SolverProgress::CacheHit { remaining_moves, .. } => {
                format!("命中缓存解，还剩 {} 步", remaining_moves)
            }
            SolverProgress::CacheMiss { .. } => "当前局面不在缓存路径上，重新搜索".to_string(),
            SolverProgress::Progress { nodes_explored, node_limit } => {
                format!("已搜索 {} / {} 个状态", nodes_explored, node_limit)
            }
            SolverProgress::Finished { solution_len, .. } => {
                format!("找到解了，共 {} 步", solution_len)
            }
            SolverProgress::Failed { .. } => "没有找到可用解".to_string(),
        };
        self.solving_progress = progress.percent();
        self.solving_frame = self.solving_frame.wrapping_add(1);
    }
    fn show_solver_failed(&mut self) {
        self.solver_failed_until = Some(Instant::now() + Duration::from_millis(1600));
    }
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
    fn push_events(&mut self, events: Vec<GameEvent>) {
        self.anim_queue.extend(events);
    }

    fn tick(&mut self) {
        if self
            .solver_failed_until
            .map(|until| Instant::now() >= until)
            .unwrap_or(false)
        {
            self.solver_failed_until = None;
        }

        // Step 1: Advance current animation timeout
        if let Some(anim) = &self.current_anim {
            if anim.start_time.elapsed() >= anim.duration {
                // Animation finished: apply event permanently to our visual board tracker
                if let Some(board) = &mut self.anim_board {
                    board.apply_event(&anim.event);
                }
                self.current_anim = None;
            }
        }

        // Step 2: Pick next animation from queue
        // We use a while loop so that if speed is Off, we can drain multiple events synchronously
        while self.current_anim.is_none() {
            if let Some(event) = self.anim_queue.pop_front() {
                let scale = self.anim_speed.scale();
                // Immediate events with no visual delay
                if matches!(event, GameEvent::Won) || scale == 0.0 {
                    if let Some(board) = &mut self.anim_board {
                        board.apply_event(&event);
                    }
                } else {
                    let base_ms = match &event {
                        GameEvent::CardMoved { .. } => 150.0,
                        GameEvent::StackMoved { .. } => 200.0,
                        GameEvent::DragonsMerged { .. } => 300.0,
                        GameEvent::Dealt { .. } | GameEvent::RestoreDealt { .. } => 800.0,
                        _ => 0.0,
                    };
                    let duration = Duration::from_secs_f32((base_ms / 1000.0) * scale);
                    self.current_anim = Some(ActiveAnimation {
                        event,
                        start_time: Instant::now(),
                        duration,
                    });
                    break;
                }
            } else {
                break;
            }
        }
    }
}

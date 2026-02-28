use serde::{Serialize, Deserialize};

/// Suits used in SHENZHEN I/O Solitaire.
/// There are three suits: Red (红), Green (绿), Black (黑).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Suit {
    Red,
    Green,
    Black,
}

impl Suit {
    /// All three suits, in canonical order.
    pub const ALL: [Suit; 3] = [Suit::Red, Suit::Green, Suit::Black];

    /// Single-character symbol used in CLI rendering.
    pub fn symbol(self) -> &'static str {
        match self {
            Suit::Red => "R",
            Suit::Green => "G",
            Suit::Black => "B",
        }
    }

    /// Full name (reserved for TUI/display use).
    #[allow(dead_code)]
    pub fn name(self) -> &'static str {
        match self {
            Suit::Red => "Red",
            Suit::Green => "Green",
            Suit::Black => "Black",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Card {
    /// A numbered card, value is 1..=9.
    Numbered(Suit, u8),
    /// A dragon card of a given suit.
    Dragon(Suit),
    /// The unique flower card.
    Flower,
}

impl Card {
    pub fn can_stack_on(self, other: Card) -> bool {
        match (self, other) {
            (Card::Numbered(s1, v1), Card::Numbered(s2, v2)) => {
                s1 != s2 && v1 + 1 == v2
            }
            _ => false,
        }
    }

    #[allow(dead_code)]
    pub fn is_dragon(self) -> bool {
        matches!(self, Card::Dragon(_))
    }

    #[allow(dead_code)]
    pub fn is_flower(self) -> bool {
        matches!(self, Card::Flower)
    }

    #[allow(dead_code)]
    pub fn is_numbered(self) -> bool {
        matches!(self, Card::Numbered(_, _))
    }

    #[allow(dead_code)]
    pub fn suit(self) -> Option<Suit> {
        match self {
            Card::Numbered(s, _) | Card::Dragon(s) => Some(s),
            Card::Flower => None,
        }
    }

    #[allow(dead_code)]
    pub fn value(self) -> Option<u8> {
        match self {
            Card::Numbered(_, v) => Some(v),
            _ => None,
        }
    }

    pub fn label(self) -> String {
        match self {
            Card::Numbered(s, v) => format!("{}{}", s.symbol(), v),
            Card::Dragon(s) => format!("{}D", s.symbol()),
            Card::Flower => "FL".to_string(),
        }
    }
}

pub fn full_deck() -> Vec<Card> {
    let mut deck = Vec::with_capacity(40);

    for &suit in &Suit::ALL {
        for v in 1..=9 {
            deck.push(Card::Numbered(suit, v));
        }
        for _ in 0..4 {
            deck.push(Card::Dragon(suit));
        }
    }

    deck.push(Card::Flower);

    debug_assert_eq!(deck.len(), 40, "Deck must have exactly 40 cards");
    deck
}

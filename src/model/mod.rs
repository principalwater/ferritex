use serde::{Deserialize, Serialize};

/// Full intermediate representation of a parsed LaTeX document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Document {
    /// Top-level blocks in document order.
    pub blocks: Vec<Block>,
}

/// Structural block-level element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Block {
    /// Section heading with optional nesting level.
    Section(Section),
    /// Plain paragraph.
    Paragraph(Paragraph),
}

/// Section node with title and hierarchy level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    /// 1-based section level.
    pub level: u8,
    /// Human-readable section title.
    pub title: String,
}

/// Paragraph with inline content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Paragraph {
    /// Ordered inline elements.
    pub inlines: Vec<Inline>,
}

/// Inline text elements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Inline {
    /// Raw text span.
    Text(String),
    /// Bold text span.
    Bold(String),
    /// Italic text span.
    Italic(String),
}

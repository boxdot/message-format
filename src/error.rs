use displaydoc::Display;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Select,
    Plural,
    Ordinal,
}

impl std::fmt::Display for BlockKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockKind::Select => write!(f, "select"),
            BlockKind::Plural => write!(f, "plural"),
            BlockKind::Ordinal => write!(f, "ordinal"),
        }
    }
}

/// Parse error for ICU message format patterns.
#[derive(Debug, Display, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseError {
    /// unmatched closing brace '}}' with no matching '{{'
    UnmatchedClosingBrace,

    /// unclosed '{{' in pattern
    UnclosedBrace,

    /// unknown block type: '{_0}'
    UnknownBlockType(String),

    /// missing value element in {block} block
    MissingBlockValue { block: BlockKind },

    /// missing 'other' clause in {block} block
    MissingOtherClause { block: BlockKind },
}

impl std::error::Error for ParseError {}

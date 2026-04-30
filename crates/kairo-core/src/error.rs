use std::fmt;

/// Errors returned while parsing or constructing Kairo identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdError {
    Empty,
    TooLong { max: usize },
    InvalidEncoding,
    InvalidReference,
    UnsupportedReferenceKind,
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("empty identifier"),
            Self::TooLong { max } => write!(f, "identifier exceeds maximum length of {max}"),
            Self::InvalidEncoding => f.write_str("invalid identifier encoding"),
            Self::InvalidReference => f.write_str("invalid reference syntax"),
            Self::UnsupportedReferenceKind => f.write_str("unsupported reference kind"),
        }
    }
}

impl std::error::Error for IdError {}

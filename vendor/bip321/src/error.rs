use std::fmt;

/// Errors that can occur when parsing a BIP-321 URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Input is too short to be a valid URI.
    TooShort,
    /// The URI scheme is not `bitcoin:`.
    InvalidScheme,
    /// The address could not be parsed.
    InvalidAddress(String),
    /// The `amount` parameter is invalid.
    InvalidAmount(String),
    /// A required parameter (`req-*`) is not recognized.
    UnknownRequiredParameter(String),
    /// A parameter key appeared more than once.
    DuplicateParameter(String),
    /// No payment instruction: no address, no lightning invoice, and no BOLT12 offer.
    MissingPaymentInstruction,
    /// Percent-decoding failed.
    PercentDecode,
    /// A query parameter is missing `=`.
    MissingEquals,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::TooShort => write!(f, "URI is too short"),
            Error::InvalidScheme => write!(f, "invalid URI scheme (expected \"bitcoin:\")"),
            Error::InvalidAddress(e) => write!(f, "invalid address: {e}"),
            Error::InvalidAmount(e) => write!(f, "invalid amount: {e}"),
            Error::UnknownRequiredParameter(k) => {
                write!(f, "unknown required parameter: {k}")
            }
            Error::DuplicateParameter(k) => write!(f, "duplicate parameter: {k}"),
            Error::MissingPaymentInstruction => {
                write!(f, "no payment instruction (address, lightning, or lno required)")
            }
            Error::PercentDecode => write!(f, "percent-decoding failed"),
            Error::MissingEquals => write!(f, "query parameter missing '='"),
        }
    }
}

impl std::error::Error for Error {}

use std::borrow::Cow;
use std::fmt;

use percent_encoding::percent_decode_str;

use crate::Error;

/// A zero-copy URI parameter value with lazy percent-decoding.
///
/// The value is stored in its original (possibly percent-encoded) form and
/// decoded on demand.
#[derive(Debug, Clone)]
pub struct Param<'a>(ParamInner<'a>);

#[derive(Debug, Clone)]
enum ParamInner<'a> {
    /// Raw borrowed slice — no percent-encoding present.
    Borrowed(&'a str),
    /// Owned decoded string (from percent-decoding).
    Decoded(String),
}

impl<'a> Param<'a> {
    /// Create a `Param` from a raw (possibly percent-encoded) string.
    ///
    /// If the string contains percent-encoded sequences they are decoded eagerly.
    /// If no encoding is present the original slice is borrowed.
    pub fn from_encoded(raw: &'a str) -> Result<Self, Error> {
        let decoded = percent_decode_str(raw)
            .decode_utf8()
            .map_err(|_| Error::PercentDecode)?;
        match decoded {
            Cow::Borrowed(_) => Ok(Param(ParamInner::Borrowed(raw))),
            Cow::Owned(s) => Ok(Param(ParamInner::Decoded(s))),
        }
    }

    /// Create a `Param` from an already-decoded string.
    pub fn from_decoded(s: String) -> Self {
        Param(ParamInner::Decoded(s))
    }

    /// Create a `Param` borrowing from an already-decoded `&str`.
    pub fn from_str_borrowed(s: &'a str) -> Self {
        Param(ParamInner::Borrowed(s))
    }

    /// Get the decoded value as a `&str`.
    pub fn as_str(&self) -> &str {
        match &self.0 {
            ParamInner::Borrowed(s) => s,
            ParamInner::Decoded(s) => s.as_str(),
        }
    }

    /// Consume self and return an owned `String`.
    pub fn into_string(self) -> String {
        match self.0 {
            ParamInner::Borrowed(s) => s.to_owned(),
            ParamInner::Decoded(s) => s,
        }
    }
}

impl<'a> fmt::Display for Param<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'a> AsRef<str> for Param<'a> {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl<'a> PartialEq for Param<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl<'a> Eq for Param<'a> {}

impl<'a> PartialEq<str> for Param<'a> {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl<'a> PartialEq<&str> for Param<'a> {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_value() {
        let p = Param::from_encoded("hello").unwrap();
        assert_eq!(p.as_str(), "hello");
    }

    #[test]
    fn percent_encoded_value() {
        let p = Param::from_encoded("hello%20world").unwrap();
        assert_eq!(p.as_str(), "hello world");
    }

    #[test]
    fn display() {
        let p = Param::from_encoded("caf%C3%A9").unwrap();
        assert_eq!(format!("{p}"), "caf\u{e9}");
    }

    #[test]
    fn equality() {
        let a = Param::from_encoded("hello%20world").unwrap();
        let b = Param::from_decoded("hello world".into());
        assert_eq!(a, b);
    }
}

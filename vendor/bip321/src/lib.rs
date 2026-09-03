//! # bip321
//!
//! A BIP-321 compliant `bitcoin:` URI parser and serializer.
//!
//! BIP-321 extends BIP-21 with:
//! - Optional address (payment info can come from `lightning` or `lno` params)
//! - Case-insensitive parameter keys
//! - Duplicate parameter detection
//! - `req-*` parameter validation
//!
//! # Examples
//!
//! ```
//! use bip321::Uri;
//!
//! // Simple address-only URI
//! let uri = Uri::parse("bitcoin:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa").unwrap();
//! assert!(uri.address.is_some());
//!
//! // Address-less URI with lightning invoice
//! let uri = Uri::parse("bitcoin:?lightning=lnbc1234").unwrap();
//! assert!(uri.address.is_none());
//! assert_eq!(uri.lightning[0].as_str(), "lnbc1234");
//! ```

mod de;
mod error;
mod param;
mod ser;

pub use error::Error;
pub use param::Param;

use bitcoin::address::NetworkUnchecked;
use bitcoin::Address;

/// Marker type for URIs with no extension parameters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoExtras;

/// A parsed BIP-321 `bitcoin:` URI.
///
/// The `Extras` type parameter allows extending the URI with custom
/// (non-`req-*`) query parameters via the [`DeserializeParams`] trait.
#[derive(Debug, Clone)]
pub struct Uri<'a, Extras = NoExtras> {
    /// The bitcoin address. `None` for address-less URIs (e.g. lightning-only).
    pub address: Option<Address<NetworkUnchecked>>,
    /// Raw address string for round-trip serialization.
    address_str: Option<String>,
    /// The requested amount in BTC.
    pub amount: Option<bitcoin::Amount>,
    /// A label for the address (e.g. name of the receiver).
    pub label: Option<Param<'a>>,
    /// A message describing the purpose of the transaction.
    pub message: Option<Param<'a>>,
    /// BOLT11 lightning invoices (payment instruction — may have multiple).
    pub lightning: Vec<Param<'a>>,
    /// BOLT12 offers (payment instruction — may have multiple).
    pub lno: Vec<Param<'a>>,
    /// Silent payment addresses (payment instruction — may have multiple).
    pub sp: Vec<Param<'a>>,
    /// On-chain bech32 addresses (can have multiple values).
    pub bc: Vec<Param<'a>>,
    /// Testnet bech32 addresses (can have multiple values).
    pub tb: Vec<Param<'a>>,
    /// Proof-of-payment callback URL.
    pub pop: Option<Param<'a>>,
    /// Whether proof-of-payment is required (`req-pop` was present).
    pub req_pop: bool,
    /// Extension parameters.
    pub extras: Extras,
}

impl<'a, Extras: Default> Uri<'a, Extras> {
    /// Create an empty URI for programmatic construction.
    pub fn new() -> Self {
        Self {
            address: None,
            address_str: None,
            amount: None,
            label: None,
            message: None,
            lightning: Vec::new(),
            lno: Vec::new(),
            sp: Vec::new(),
            bc: Vec::new(),
            tb: Vec::new(),
            pop: None,
            req_pop: false,
            extras: Extras::default(),
        }
    }

    /// Set the on-chain address (both parsed and raw string for serialization).
    pub fn set_address(&mut self, raw: String, parsed: Address<NetworkUnchecked>) {
        self.address_str = Some(raw);
        self.address = Some(parsed);
    }
}

impl<'a, Extras> Uri<'a, Extras> {
    /// Returns the raw address string, if an address was present in the URI.
    pub fn address_str(&self) -> Option<&str> {
        self.address_str.as_deref()
    }
}

impl<'a> Uri<'a, NoExtras> {
    /// Parse a BIP-321 URI string.
    pub fn parse(s: &'a str) -> Result<Self, Error> {
        de::parse(s)
    }
}

impl<'a, Extras: DeserializeParams<'a>> Uri<'a, Extras> {
    /// Parse a BIP-321 URI string with custom extension parameters.
    pub fn parse_with_extras(s: &'a str) -> Result<Self, Error> {
        de::parse_with_extras(s)
    }
}

impl<'a, Extras> std::fmt::Display for Uri<'a, Extras>
where
    Extras: SerializeParams,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        ser::serialize(self, f)
    }
}


/// Trait for deserializing custom extension parameters from a URI.
///
/// Implement this on your extras type to capture unknown (non-`req-*`)
/// query parameters.
pub trait DeserializeParams<'a>: Sized + Default {
    /// Called for each unknown query parameter.
    ///
    /// Return `Ok(())` to accept the parameter, or `Err` to reject it.
    /// Parameters with `req-` prefix that aren't handled will cause
    /// `Error::UnknownRequiredParameter` — this method is only called
    /// for non-required unknown parameters.
    fn deserialize_param(&mut self, key: &str, value: Param<'a>) -> Result<(), Error>;
}

impl<'a> DeserializeParams<'a> for NoExtras {
    fn deserialize_param(&mut self, _key: &str, _value: Param<'a>) -> Result<(), Error> {
        // Silently ignore unknown non-required parameters
        Ok(())
    }
}

/// Trait for serializing custom extension parameters into a URI.
pub trait SerializeParams {
    /// Write extension parameters as `&key=value` pairs to the formatter.
    fn serialize_params(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result;
}

impl SerializeParams for NoExtras {
    fn serialize_params(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

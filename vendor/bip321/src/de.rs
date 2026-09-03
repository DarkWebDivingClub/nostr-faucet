use std::collections::HashSet;

use bitcoin::address::NetworkUnchecked;
use bitcoin::Amount;

use crate::error::Error;
use crate::param::Param;
use crate::{DeserializeParams, NoExtras, Uri};

/// Minimum valid URI: `bitcoin:` (8 chars) — address-less requires query params
/// but we check that later.
const MIN_LEN: usize = 8;

/// The set of known parameter keys (lowercase).
const KNOWN_KEYS: &[&str] = &[
    "amount", "label", "message", "lightning", "lno", "sp", "bc", "tb", "pop", "req-pop",
];

/// Payment instruction keys — allowed to appear multiple times per BIP-321.
const MULTI_VALUE_KEYS: &[&str] = &["lightning", "lno", "sp", "bc", "tb"];

/// Parse a BIP-321 URI with no extension parameters.
pub(crate) fn parse<'a>(s: &'a str) -> Result<Uri<'a, NoExtras>, Error> {
    parse_with_extras(s)
}

/// Parse a BIP-321 URI with custom extension parameters.
pub(crate) fn parse_with_extras<'a, Extras: DeserializeParams<'a>>(
    s: &'a str,
) -> Result<Uri<'a, Extras>, Error> {
    if s.len() < MIN_LEN {
        return Err(Error::TooShort);
    }

    // Scheme check (case-insensitive per BIP-321)
    if !s[..8].eq_ignore_ascii_case("bitcoin:") {
        return Err(Error::InvalidScheme);
    }

    let after_scheme = &s[8..];

    // Split address from query string
    let (addr_str, query) = match after_scheme.find('?') {
        Some(pos) => (&after_scheme[..pos], Some(&after_scheme[pos + 1..])),
        None => (after_scheme, None),
    };

    // Parse address (optional in BIP-321)
    let (address, address_str): (Option<bitcoin::Address<NetworkUnchecked>>, Option<String>) = if addr_str.is_empty() {
        (None, None)
    } else {
        let parsed: bitcoin::Address<NetworkUnchecked> = addr_str
            .parse()
            .map_err(|e: bitcoin::address::ParseError| Error::InvalidAddress(e.to_string()))?;
        (Some(parsed), Some(addr_str.to_owned()))
    };

    let mut amount: Option<Amount> = None;
    let mut label: Option<Param<'a>> = None;
    let mut message: Option<Param<'a>> = None;
    let mut lightning: Vec<Param<'a>> = Vec::new();
    let mut lno: Vec<Param<'a>> = Vec::new();
    let mut sp: Vec<Param<'a>> = Vec::new();
    let mut bc: Vec<Param<'a>> = Vec::new();
    let mut tb: Vec<Param<'a>> = Vec::new();
    let mut pop: Option<Param<'a>> = None;
    let mut req_pop = false;
    let mut extras = Extras::default();

    if let Some(query) = query {
        let mut seen = HashSet::new();

        for pair in query.split('&') {
            if pair.is_empty() {
                continue;
            }

            let (raw_key, raw_value) = pair.split_once('=').ok_or(Error::MissingEquals)?;

            // BIP-321: keys are case-insensitive
            let key_lower = raw_key.to_ascii_lowercase();

            // Duplicate detection (bc/tb are allowed to repeat)
            let is_multi = MULTI_VALUE_KEYS.contains(&key_lower.as_str());
            if !is_multi && !seen.insert(key_lower.clone()) {
                return Err(Error::DuplicateParameter(key_lower));
            }

            match key_lower.as_str() {
                "amount" => {
                    amount = Some(parse_amount(raw_value)?);
                }
                "label" => {
                    label = Some(Param::from_encoded(raw_value)?);
                }
                "message" => {
                    message = Some(Param::from_encoded(raw_value)?);
                }
                "lightning" => {
                    lightning.push(Param::from_encoded(raw_value)?);
                }
                "lno" => {
                    lno.push(Param::from_encoded(raw_value)?);
                }
                "sp" => {
                    sp.push(Param::from_encoded(raw_value)?);
                }
                "bc" => {
                    bc.push(Param::from_encoded(raw_value)?);
                }
                "tb" => {
                    tb.push(Param::from_encoded(raw_value)?);
                }
                "pop" => {
                    pop = Some(Param::from_encoded(raw_value)?);
                }
                "req-pop" => {
                    req_pop = true;
                    if !raw_value.is_empty() {
                        pop = Some(Param::from_encoded(raw_value)?);
                    }
                }
                other => {
                    if let Some(stripped) = other.strip_prefix("req-") {
                        if !is_known_key(other) {
                            return Err(Error::UnknownRequiredParameter(stripped.to_owned()));
                        }
                    }
                    let value = Param::from_encoded(raw_value)?;
                    extras.deserialize_param(other, value)?;
                }
            }
        }
    }

    // BIP-321: must have at least one payment instruction
    let has_payment = address.is_some()
        || !lightning.is_empty()
        || !lno.is_empty()
        || !sp.is_empty()
        || !bc.is_empty()
        || !tb.is_empty();
    if !has_payment {
        return Err(Error::MissingPaymentInstruction);
    }

    Ok(Uri {
        address,
        address_str,
        amount,
        label,
        message,
        lightning,
        lno,
        sp,
        bc,
        tb,
        pop,
        req_pop,
        extras,
    })
}

/// Parse an amount string per BIP-321.
///
/// The amount is in BTC, must not be negative, and must not use
/// more than 8 decimal places.
fn parse_amount(s: &str) -> Result<Amount, Error> {
    if s.is_empty() {
        return Err(Error::InvalidAmount("empty".into()));
    }

    // BIP-21/321: amount is in BTC (not satoshis)
    let btc: f64 = s.parse().map_err(|_| Error::InvalidAmount(s.into()))?;

    if btc < 0.0 {
        return Err(Error::InvalidAmount("negative amount".into()));
    }

    // Convert to satoshis (1 BTC = 100_000_000 sat)
    // Use string-based parsing to avoid floating-point precision issues
    Amount::from_str_in(s, bitcoin::Denomination::Bitcoin)
        .map_err(|e| Error::InvalidAmount(e.to_string()))
}

fn is_known_key(key: &str) -> bool {
    KNOWN_KEYS.contains(&key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn too_short() {
        assert_eq!(parse("bitcoin").unwrap_err(), Error::TooShort);
    }

    #[test]
    fn wrong_scheme() {
        assert_eq!(
            parse("litecoin:abc").unwrap_err(),
            Error::InvalidScheme
        );
    }

    #[test]
    fn simple_address() {
        let uri = parse("bitcoin:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa").unwrap();
        assert!(uri.address.is_some());
        assert!(uri.amount.is_none());
    }

    #[test]
    fn address_with_amount() {
        let uri = parse(
            "bitcoin:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa?amount=0.001",
        )
        .unwrap();
        assert!(uri.address.is_some());
        assert_eq!(
            uri.amount.unwrap(),
            Amount::from_sat(100_000)
        );
    }

    #[test]
    fn case_insensitive_scheme() {
        let uri = parse("BITCOIN:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa").unwrap();
        assert!(uri.address.is_some());
    }

    #[test]
    fn case_insensitive_keys() {
        let uri = parse(
            "bitcoin:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa?AMOUNT=0.001&Label=test",
        )
        .unwrap();
        assert!(uri.amount.is_some());
        assert_eq!(uri.label.unwrap().as_str(), "test");
    }

    #[test]
    fn address_less_lightning() {
        let uri = parse("bitcoin:?lightning=lnbc1234").unwrap();
        assert!(uri.address.is_none());
        assert_eq!(uri.lightning[0].as_str(), "lnbc1234");
    }

    #[test]
    fn address_less_no_instruction() {
        assert_eq!(
            parse("bitcoin:?label=test").unwrap_err(),
            Error::MissingPaymentInstruction
        );
    }

    #[test]
    fn duplicate_key() {
        assert!(matches!(
            parse("bitcoin:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa?amount=1&amount=2"),
            Err(Error::DuplicateParameter(k)) if k == "amount"
        ));
    }

    #[test]
    fn unknown_required_param() {
        assert!(matches!(
            parse("bitcoin:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa?req-foo=bar"),
            Err(Error::UnknownRequiredParameter(k)) if k == "foo"
        ));
    }

    #[test]
    fn missing_equals() {
        assert_eq!(
            parse("bitcoin:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa?noeq").unwrap_err(),
            Error::MissingEquals
        );
    }

    #[test]
    fn percent_encoded_label() {
        let uri = parse(
            "bitcoin:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa?label=Luke-%2DJr",
        )
        .unwrap();
        assert_eq!(uri.label.unwrap().as_str(), "Luke--Jr");
    }

    #[test]
    fn combined_lightning_and_address() {
        let uri = parse(
            "bitcoin:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa?lightning=lnbc1234&amount=0.01",
        )
        .unwrap();
        assert!(uri.address.is_some());
        assert!(!uri.lightning.is_empty());
        assert!(uri.amount.is_some());
    }
}

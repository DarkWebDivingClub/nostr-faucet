use std::fmt;

use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};

use crate::param::Param;
use crate::{SerializeParams, Uri};

/// Serialize a `Uri` into a valid `bitcoin:` URI string.
pub(crate) fn serialize<Extras: SerializeParams>(
    uri: &Uri<'_, Extras>,
    f: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    write!(f, "bitcoin:")?;

    if let Some(ref addr_str) = uri.address_str {
        write!(f, "{addr_str}")?;
    }

    let mut sep = QuerySep::new();

    if let Some(ref amount) = uri.amount {
        // Format as BTC with up to 8 decimal places, trimming trailing zeros
        let btc = format!("{}", amount.to_btc());
        write!(f, "{}amount={}", sep.next(), btc)?;
    }

    if let Some(ref label) = uri.label {
        write!(f, "{}label={}", sep.next(), encode_param(label))?;
    }

    if let Some(ref message) = uri.message {
        write!(f, "{}message={}", sep.next(), encode_param(message))?;
    }

    for inv in &uri.lightning {
        write!(f, "{}lightning={}", sep.next(), encode_param(inv))?;
    }

    for offer in &uri.lno {
        write!(f, "{}lno={}", sep.next(), encode_param(offer))?;
    }

    for addr in &uri.sp {
        write!(f, "{}sp={}", sep.next(), encode_param(addr))?;
    }

    for addr in &uri.bc {
        write!(f, "{}bc={}", sep.next(), encode_param(addr))?;
    }

    for addr in &uri.tb {
        write!(f, "{}tb={}", sep.next(), encode_param(addr))?;
    }

    if uri.req_pop {
        if let Some(ref pop) = uri.pop {
            write!(f, "{}req-pop={}", sep.next(), encode_param(pop))?;
        } else {
            write!(f, "{}req-pop=", sep.next())?;
        }
    } else if let Some(ref pop) = uri.pop {
        write!(f, "{}pop={}", sep.next(), encode_param(pop))?;
    }

    uri.extras.serialize_params(f)?;

    Ok(())
}

/// Percent-encode a parameter value.
fn encode_param(param: &Param<'_>) -> String {
    utf8_percent_encode(param.as_str(), NON_ALPHANUMERIC).to_string()
}

/// Helper to emit `?` for the first query parameter and `&` for subsequent ones.
struct QuerySep {
    first: bool,
}

impl QuerySep {
    fn new() -> Self {
        QuerySep { first: true }
    }

    fn next(&mut self) -> char {
        if self.first {
            self.first = false;
            '?'
        } else {
            '&'
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn serialize_address_only() {
        let uri = crate::de::parse(
            "bitcoin:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa",
        )
        .unwrap();
        let s = format!("{uri}");
        assert_eq!(s, "bitcoin:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa");
    }

    #[test]
    fn serialize_with_amount() {
        let uri = crate::de::parse(
            "bitcoin:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa?amount=0.001",
        )
        .unwrap();
        let s = format!("{uri}");
        assert!(s.contains("amount=0.001"));
    }

    #[test]
    fn serialize_lightning_only() {
        let uri = crate::de::parse("bitcoin:?lightning=lnbc1234").unwrap();
        let s = format!("{uri}");
        assert_eq!(s, "bitcoin:?lightning=lnbc1234");
    }
}

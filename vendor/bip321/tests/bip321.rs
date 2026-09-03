use bip321::{Error, Param, Uri};
use bitcoin::address::NetworkUnchecked;
use bitcoin::{Address, Amount};

// Valid mainnet P2PKH address (genesis block coinbase)
const ADDR: &str = "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa";

// Valid bech32 segwit addresses
const BECH32_P2WPKH: &str = "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4";
const BECH32_TAPROOT: &str = "bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqzk5jj0";

// =========================================================================
// BIP-321 Spec Examples — Valid URIs
// =========================================================================

#[test]
fn spec_address_only() {
    let s = format!("bitcoin:{ADDR}");
    let uri = Uri::parse(&s).unwrap();
    assert!(uri.address.is_some());
    assert!(uri.amount.is_none());
    assert!(uri.label.is_none());
    assert!(uri.message.is_none());
}

#[test]
fn spec_address_with_label() {
    let s = format!("bitcoin:{ADDR}?label=Luke-Jr");
    let uri = Uri::parse(&s).unwrap();
    assert_eq!(uri.label.as_ref().unwrap().as_str(), "Luke-Jr");
}

#[test]
fn spec_address_with_amount_and_label() {
    let s = format!("bitcoin:{ADDR}?amount=20.3&label=Luke-Jr");
    let uri = Uri::parse(&s).unwrap();
    assert_eq!(uri.amount.unwrap(), Amount::from_btc(20.3).unwrap());
    assert_eq!(uri.label.as_ref().unwrap().as_str(), "Luke-Jr");
}

#[test]
fn spec_address_with_amount_label_message() {
    let s = format!(
        "bitcoin:{ADDR}?amount=50&label=Luke-Jr&message=Donation%20for%20project%20xyz"
    );
    let uri = Uri::parse(&s).unwrap();
    assert_eq!(uri.amount.unwrap(), Amount::from_btc(50.0).unwrap());
    assert_eq!(uri.label.as_ref().unwrap().as_str(), "Luke-Jr");
    assert_eq!(
        uri.message.as_ref().unwrap().as_str(),
        "Donation for project xyz"
    );
}

#[test]
fn spec_address_with_lightning() {
    let s = format!("bitcoin:{ADDR}?lightning=lnbc420bogusinvoice");
    let uri = Uri::parse(&s).unwrap();
    assert!(uri.address.is_some());
    assert_eq!(uri.lightning[0].as_str(), "lnbc420bogusinvoice");
}

#[test]
fn spec_lightning_only() {
    let uri = Uri::parse("bitcoin:?lightning=lnbc420bogusinvoice").unwrap();
    assert!(uri.address.is_none());
    assert_eq!(uri.lightning[0].as_str(), "lnbc420bogusinvoice");
}

#[test]
fn spec_lno_only() {
    let uri = Uri::parse("bitcoin:?lno=lno1bogusoffer").unwrap();
    assert!(uri.address.is_none());
    assert_eq!(uri.lno[0].as_str(), "lno1bogusoffer");
}

#[test]
fn spec_lno_with_sp() {
    let uri = Uri::parse("bitcoin:?lno=lno1bogusoffer&sp=sp1qsilentpayment").unwrap();
    assert_eq!(uri.lno[0].as_str(), "lno1bogusoffer");
    assert_eq!(uri.sp[0].as_str(), "sp1qsilentpayment");
}

#[test]
fn spec_sp_only() {
    let uri = Uri::parse("bitcoin:?sp=sp1qsilentpayment").unwrap();
    assert!(uri.address.is_none());
    assert_eq!(uri.sp[0].as_str(), "sp1qsilentpayment");
}

#[test]
fn spec_address_with_sp() {
    let s = format!("bitcoin:{ADDR}?sp=sp1qsilentpayment");
    let uri = Uri::parse(&s).unwrap();
    assert!(uri.address.is_some());
    assert_eq!(uri.sp[0].as_str(), "sp1qsilentpayment");
}

#[test]
fn spec_unknown_non_required_params_ok() {
    let s = format!(
        "bitcoin:{ADDR}?somethingyoudontunderstand=50&somethingelseyoudontget=999"
    );
    let uri = Uri::parse(&s).unwrap();
    assert!(uri.address.is_some());
}

#[test]
fn spec_multiple_bc_params() {
    let s = format!("bitcoin:?bc={BECH32_P2WPKH}&bc={BECH32_TAPROOT}");
    let uri = Uri::parse(&s).unwrap();
    assert!(uri.address.is_none());
    assert_eq!(uri.bc.len(), 2);
    assert_eq!(uri.bc[0].as_str(), BECH32_P2WPKH);
    assert_eq!(uri.bc[1].as_str(), BECH32_TAPROOT);
}

#[test]
fn spec_uppercase_scheme_and_address() {
    let upper = BECH32_P2WPKH.to_uppercase();
    let s = format!("BITCOIN:{upper}");
    let uri = Uri::parse(&s).unwrap();
    assert!(uri.address.is_some());
}

#[test]
fn spec_uppercase_bc_params() {
    let upper_p2wpkh = BECH32_P2WPKH.to_uppercase();
    let upper_taproot = BECH32_TAPROOT.to_uppercase();
    let s = format!("BITCOIN:?BC={upper_p2wpkh}&BC={upper_taproot}");
    let uri = Uri::parse(&s).unwrap();
    assert_eq!(uri.bc.len(), 2);
}

#[test]
fn spec_tb_param() {
    let uri = Uri::parse("bitcoin:?tb=tb1qghfhmd4zh7ncpmxl3qzhmq566jk8ckq4gafnmg").unwrap();
    assert_eq!(uri.tb.len(), 1);
    assert_eq!(
        uri.tb[0].as_str(),
        "tb1qghfhmd4zh7ncpmxl3qzhmq566jk8ckq4gafnmg"
    );
}

// =========================================================================
// BIP-321 Spec Examples — Invalid URIs
// =========================================================================

#[test]
fn spec_invalid_duplicate_label() {
    let s = format!("bitcoin:{ADDR}?label=Luke-Jr&label=Matt");
    let err = Uri::parse(&s).unwrap_err();
    assert!(matches!(err, Error::DuplicateParameter(k) if k == "label"));
}

#[test]
fn spec_invalid_duplicate_amount() {
    let s = format!("bitcoin:{ADDR}?amount=42&amount=10");
    let err = Uri::parse(&s).unwrap_err();
    assert!(matches!(err, Error::DuplicateParameter(k) if k == "amount"));
}

#[test]
fn spec_invalid_duplicate_amount_same_value() {
    let s = format!("bitcoin:{ADDR}?amount=42&amount=42");
    let err = Uri::parse(&s).unwrap_err();
    assert!(matches!(err, Error::DuplicateParameter(k) if k == "amount"));
}

#[test]
fn spec_invalid_unknown_required_params() {
    let s = format!(
        "bitcoin:{ADDR}?req-somethingyoudontunderstand=50&req-somethingelseyoudontget=999"
    );
    let err = Uri::parse(&s).unwrap_err();
    assert!(matches!(err, Error::UnknownRequiredParameter(_)));
}

// =========================================================================
// Case sensitivity
// =========================================================================

#[test]
fn case_insensitive_scheme_lowercase() {
    let s = format!("bitcoin:{ADDR}");
    Uri::parse(&s).unwrap();
}

#[test]
fn case_insensitive_scheme_uppercase() {
    let s = format!("BITCOIN:{ADDR}");
    Uri::parse(&s).unwrap();
}

#[test]
fn case_insensitive_scheme_mixed() {
    let s = format!("BiTcOiN:{ADDR}");
    Uri::parse(&s).unwrap();
}

#[test]
fn case_insensitive_param_keys() {
    let s = format!("bitcoin:{ADDR}?AMOUNT=1&LABEL=Test&MESSAGE=Hello&LIGHTNING=lnbc1");
    let uri = Uri::parse(&s).unwrap();
    assert!(uri.amount.is_some());
    assert_eq!(uri.label.as_ref().unwrap().as_str(), "Test");
    assert_eq!(uri.message.as_ref().unwrap().as_str(), "Hello");
    assert_eq!(uri.lightning[0].as_str(), "lnbc1");
}

#[test]
fn case_insensitive_mixed_case_keys() {
    let s = format!("bitcoin:{ADDR}?Amount=1&LaBeL=Bob");
    let uri = Uri::parse(&s).unwrap();
    assert!(uri.amount.is_some());
    assert_eq!(uri.label.as_ref().unwrap().as_str(), "Bob");
}

// =========================================================================
// Amount parsing
// =========================================================================

#[test]
fn amount_integer() {
    let s = format!("bitcoin:{ADDR}?amount=50");
    let uri = Uri::parse(&s).unwrap();
    assert_eq!(uri.amount.unwrap(), Amount::from_btc(50.0).unwrap());
}

#[test]
fn amount_decimal() {
    let s = format!("bitcoin:{ADDR}?amount=0.00000001");
    let uri = Uri::parse(&s).unwrap();
    assert_eq!(uri.amount.unwrap(), Amount::from_sat(1));
}

#[test]
fn amount_with_trailing_zeros() {
    let s = format!("bitcoin:{ADDR}?amount=1.00000000");
    let uri = Uri::parse(&s).unwrap();
    assert_eq!(uri.amount.unwrap(), Amount::from_btc(1.0).unwrap());
}

#[test]
fn amount_zero() {
    let s = format!("bitcoin:{ADDR}?amount=0");
    let uri = Uri::parse(&s).unwrap();
    assert_eq!(uri.amount.unwrap(), Amount::from_sat(0));
}

#[test]
fn amount_negative_rejected() {
    let s = format!("bitcoin:{ADDR}?amount=-1");
    let err = Uri::parse(&s).unwrap_err();
    assert!(matches!(err, Error::InvalidAmount(_)));
}

#[test]
fn amount_empty_rejected() {
    let s = format!("bitcoin:{ADDR}?amount=");
    let err = Uri::parse(&s).unwrap_err();
    assert!(matches!(err, Error::InvalidAmount(_)));
}

#[test]
fn amount_non_numeric_rejected() {
    let s = format!("bitcoin:{ADDR}?amount=abc");
    let err = Uri::parse(&s).unwrap_err();
    assert!(matches!(err, Error::InvalidAmount(_)));
}

// =========================================================================
// Percent encoding
// =========================================================================

#[test]
fn percent_encoded_spaces() {
    let s = format!("bitcoin:{ADDR}?message=Hello%20World");
    let uri = Uri::parse(&s).unwrap();
    assert_eq!(uri.message.as_ref().unwrap().as_str(), "Hello World");
}

#[test]
fn percent_encoded_special_chars() {
    // ₿ in UTF-8
    let s = format!("bitcoin:{ADDR}?label=%E2%82%BF");
    let uri = Uri::parse(&s).unwrap();
    assert_eq!(uri.label.as_ref().unwrap().as_str(), "\u{20BF}");
}

#[test]
fn percent_encoded_ampersand() {
    let s = format!("bitcoin:{ADDR}?label=foo%26bar");
    let uri = Uri::parse(&s).unwrap();
    assert_eq!(uri.label.as_ref().unwrap().as_str(), "foo&bar");
}

// =========================================================================
// Error conditions
// =========================================================================

#[test]
fn error_empty_string() {
    assert!(matches!(Uri::parse(""), Err(Error::TooShort)));
}

#[test]
fn error_too_short() {
    assert!(matches!(Uri::parse("bitcoin"), Err(Error::TooShort)));
}

#[test]
fn error_wrong_scheme() {
    assert!(matches!(
        Uri::parse("litecoin:abc123"),
        Err(Error::InvalidScheme)
    ));
}

#[test]
fn error_invalid_address() {
    assert!(matches!(
        Uri::parse("bitcoin:notavalidaddress"),
        Err(Error::InvalidAddress(_))
    ));
}

#[test]
fn error_no_payment_instruction() {
    assert!(matches!(
        Uri::parse("bitcoin:?label=test"),
        Err(Error::MissingPaymentInstruction)
    ));
}

#[test]
fn error_no_payment_instruction_empty() {
    assert!(matches!(
        Uri::parse("bitcoin:"),
        Err(Error::MissingPaymentInstruction)
    ));
}

#[test]
fn error_missing_equals_in_param() {
    let s = format!("bitcoin:{ADDR}?noeq");
    assert!(matches!(Uri::parse(&s), Err(Error::MissingEquals)));
}

#[test]
fn error_duplicate_label() {
    let s = format!("bitcoin:{ADDR}?label=a&label=b");
    assert!(matches!(
        Uri::parse(&s),
        Err(Error::DuplicateParameter(k)) if k == "label"
    ));
}

#[test]
fn error_duplicate_message() {
    let s = format!("bitcoin:{ADDR}?message=a&message=b");
    assert!(matches!(
        Uri::parse(&s),
        Err(Error::DuplicateParameter(k)) if k == "message"
    ));
}

#[test]
fn multiple_lightning_allowed() {
    let s = format!("bitcoin:{ADDR}?lightning=lnbc1&lightning=lnbc2");
    let uri = Uri::parse(&s).unwrap();
    assert_eq!(uri.lightning.len(), 2);
    assert_eq!(uri.lightning[0].as_str(), "lnbc1");
    assert_eq!(uri.lightning[1].as_str(), "lnbc2");
}

#[test]
fn error_duplicate_case_insensitive() {
    let s = format!("bitcoin:{ADDR}?amount=1&AMOUNT=2");
    assert!(matches!(
        Uri::parse(&s),
        Err(Error::DuplicateParameter(k)) if k == "amount"
    ));
}

#[test]
fn error_unknown_req_param() {
    let s = format!("bitcoin:{ADDR}?req-newfeature=1");
    assert!(matches!(
        Uri::parse(&s),
        Err(Error::UnknownRequiredParameter(k)) if k == "newfeature"
    ));
}

// =========================================================================
// Serialization round-trip
// =========================================================================

#[test]
fn roundtrip_address_only() {
    let input = format!("bitcoin:{ADDR}");
    let uri = Uri::parse(&input).unwrap();
    let output = format!("{uri}");
    assert_eq!(output, input);
}

#[test]
fn roundtrip_with_amount() {
    let input = format!("bitcoin:{ADDR}?amount=0.001");
    let uri = Uri::parse(&input).unwrap();
    let output = format!("{uri}");
    assert_eq!(output, input);
}

#[test]
fn roundtrip_lightning_only() {
    let input = "bitcoin:?lightning=lnbc1234";
    let uri = Uri::parse(input).unwrap();
    let output = format!("{uri}");
    assert_eq!(output, input);
}

#[test]
fn roundtrip_lno_only() {
    let input = "bitcoin:?lno=lno1bogusoffer";
    let uri = Uri::parse(input).unwrap();
    let output = format!("{uri}");
    assert_eq!(output, input);
}

#[test]
fn roundtrip_sp_only() {
    let input = "bitcoin:?sp=sp1qsilentpayment";
    let uri = Uri::parse(input).unwrap();
    let output = format!("{uri}");
    assert_eq!(output, input);
}

#[test]
fn roundtrip_complex() {
    let input = format!(
        "bitcoin:{ADDR}?amount=1.5&label=Satoshi&message=Donation&lightning=lnbc1234"
    );
    let uri = Uri::parse(&input).unwrap();
    let output = format!("{uri}");
    // Parse the output again to verify semantic equivalence
    let uri2 = Uri::parse(&output).unwrap();
    assert_eq!(uri.amount, uri2.amount);
    assert_eq!(
        uri.label.as_ref().unwrap().as_str(),
        uri2.label.as_ref().unwrap().as_str()
    );
    assert_eq!(
        uri.message.as_ref().unwrap().as_str(),
        uri2.message.as_ref().unwrap().as_str()
    );
    assert_eq!(uri.lightning[0].as_str(), uri2.lightning[0].as_str());
}

#[test]
fn roundtrip_bc_params() {
    let input = format!("bitcoin:?bc={BECH32_P2WPKH}&bc={BECH32_TAPROOT}");
    let uri = Uri::parse(&input).unwrap();
    let output = format!("{uri}");
    let uri2 = Uri::parse(&output).unwrap();
    assert_eq!(uri2.bc.len(), 2);
    assert_eq!(uri2.bc[0].as_str(), BECH32_P2WPKH);
    assert_eq!(uri2.bc[1].as_str(), BECH32_TAPROOT);
}

// =========================================================================
// Edge cases
// =========================================================================

#[test]
fn empty_label_value() {
    let s = format!("bitcoin:{ADDR}?label=");
    let uri = Uri::parse(&s).unwrap();
    assert_eq!(uri.label.as_ref().unwrap().as_str(), "");
}

#[test]
fn empty_message_value() {
    let s = format!("bitcoin:{ADDR}?message=");
    let uri = Uri::parse(&s).unwrap();
    assert_eq!(uri.message.as_ref().unwrap().as_str(), "");
}

#[test]
fn trailing_ampersand() {
    let s = format!("bitcoin:{ADDR}?label=test&");
    let uri = Uri::parse(&s).unwrap();
    assert_eq!(uri.label.as_ref().unwrap().as_str(), "test");
}

#[test]
fn double_ampersand() {
    let s = format!("bitcoin:{ADDR}?label=test&&amount=1");
    let uri = Uri::parse(&s).unwrap();
    assert_eq!(uri.label.as_ref().unwrap().as_str(), "test");
    assert!(uri.amount.is_some());
}

#[test]
fn bech32_address() {
    let s = format!("bitcoin:{BECH32_P2WPKH}");
    let uri = Uri::parse(&s).unwrap();
    assert!(uri.address.is_some());
}

#[test]
fn taproot_address() {
    let s = format!("bitcoin:{BECH32_TAPROOT}");
    let uri = Uri::parse(&s).unwrap();
    assert!(uri.address.is_some());
}

#[test]
fn value_with_equals_sign() {
    let s = format!("bitcoin:{ADDR}?label=a=b");
    let uri = Uri::parse(&s).unwrap();
    assert_eq!(uri.label.as_ref().unwrap().as_str(), "a=b");
}

#[test]
fn req_pop_with_value() {
    let s = format!("bitcoin:{ADDR}?req-pop=https%3A%2F%2Fexample.com%2Fcallback");
    let uri = Uri::parse(&s).unwrap();
    assert!(uri.req_pop);
    assert_eq!(
        uri.pop.as_ref().unwrap().as_str(),
        "https://example.com/callback"
    );
}

#[test]
fn req_pop_without_value() {
    let s = format!("bitcoin:{ADDR}?req-pop=");
    let uri = Uri::parse(&s).unwrap();
    assert!(uri.req_pop);
    assert!(uri.pop.is_none());
}

#[test]
fn pop_without_req() {
    let s = format!("bitcoin:{ADDR}?pop=callback");
    let uri = Uri::parse(&s).unwrap();
    assert!(!uri.req_pop);
    assert_eq!(uri.pop.as_ref().unwrap().as_str(), "callback");
}

#[test]
fn bc_duplicate_allowed() {
    // bc is allowed to have multiple values — not a duplicate error
    let s = format!("bitcoin:?bc=addr1&bc=addr2&bc=addr3");
    let uri = Uri::parse(&s).unwrap();
    assert_eq!(uri.bc.len(), 3);
}

#[test]
fn multiple_lno_allowed() {
    let uri = Uri::parse("bitcoin:?lno=offer1&lno=offer2").unwrap();
    assert_eq!(uri.lno.len(), 2);
}

#[test]
fn multiple_sp_allowed() {
    let uri = Uri::parse("bitcoin:?sp=sp1a&sp=sp1b").unwrap();
    assert_eq!(uri.sp.len(), 2);
}

#[test]
fn tb_duplicate_allowed() {
    let s = format!("bitcoin:?tb=addr1&tb=addr2");
    let uri = Uri::parse(&s).unwrap();
    assert_eq!(uri.tb.len(), 2);
}

// =========================================================================
// DeserializeParams trait — custom extras
// =========================================================================

#[derive(Debug, Default)]
struct CustomExtras {
    custom_key: Option<String>,
}

impl<'a> bip321::DeserializeParams<'a> for CustomExtras {
    fn deserialize_param(
        &mut self,
        key: &str,
        value: bip321::Param<'a>,
    ) -> Result<(), bip321::Error> {
        if key == "custom" {
            self.custom_key = Some(value.into_string());
        }
        Ok(())
    }
}

impl bip321::SerializeParams for CustomExtras {
    fn serialize_params(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(ref v) = self.custom_key {
            write!(f, "&custom={v}")?;
        }
        Ok(())
    }
}

#[test]
fn custom_extras_parsed() {
    let s = format!("bitcoin:{ADDR}?custom=myvalue");
    let uri: Uri<'_, CustomExtras> = Uri::parse_with_extras(&s).unwrap();
    assert_eq!(uri.extras.custom_key.as_deref(), Some("myvalue"));
}

#[test]
fn custom_extras_serialized() {
    let s = format!("bitcoin:{ADDR}?custom=myvalue");
    let uri: Uri<'_, CustomExtras> = Uri::parse_with_extras(&s).unwrap();
    let out = format!("{uri}");
    assert!(out.contains("custom=myvalue"));
}

// =========================================================================
// Builder (Uri::new) tests
// =========================================================================

#[test]
fn builder_address_only() {
    let mut uri: Uri<'_> = Uri::new();
    let parsed: Address<NetworkUnchecked> = ADDR.parse().unwrap();
    uri.set_address(ADDR.to_string(), parsed);
    let out = format!("{uri}");
    assert_eq!(out, format!("bitcoin:{ADDR}"));
}

#[test]
fn builder_lightning_only() {
    let mut uri: Uri<'_> = Uri::new();
    uri.lightning
        .push(Param::from_decoded("lnbc1234".to_string()));
    let out = format!("{uri}");
    assert_eq!(out, "bitcoin:?lightning=lnbc1234");
}

#[test]
fn builder_full_roundtrip() {
    let mut uri: Uri<'_> = Uri::new();
    let parsed: Address<NetworkUnchecked> = ADDR.parse().unwrap();
    uri.set_address(ADDR.to_string(), parsed);
    uri.amount = Some(Amount::from_btc(0.001).unwrap());
    uri.label = Some(Param::from_decoded("Alice".to_string()));
    uri.message = Some(Param::from_decoded("coffee".to_string()));
    uri.lightning
        .push(Param::from_decoded("lnbc1234".to_string()));
    uri.lno
        .push(Param::from_decoded("lno1offer".to_string()));

    let out = format!("{uri}");
    let reparsed = Uri::parse(&out).unwrap();
    assert_eq!(reparsed.amount.unwrap(), Amount::from_btc(0.001).unwrap());
    assert_eq!(reparsed.label.as_ref().unwrap().as_str(), "Alice");
    assert_eq!(reparsed.message.as_ref().unwrap().as_str(), "coffee");
    assert_eq!(reparsed.lightning[0].as_str(), "lnbc1234");
    assert_eq!(reparsed.lno[0].as_str(), "lno1offer");
    assert!(reparsed.address.is_some());
}

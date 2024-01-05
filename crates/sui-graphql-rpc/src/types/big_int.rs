// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::str::FromStr;

use async_graphql::*;
use move_core_types::u256::U256;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(transparent)]
pub(crate) struct BigInt(String);

#[Scalar]
impl ScalarType for BigInt {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => BigInt::from_str(&s)
                .map_err(|_| InputValueError::custom("Not a number".to_string())),
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.clone())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct NotANumber;

impl FromStr for BigInt {
    type Err = NotANumber;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut r = s;
        let mut signed = false;
        // check that all are digits and first can start with -
        if s.starts_with('-') {
            r = s.strip_prefix('-').unwrap();
            signed = true;
        }
        r = r.trim_start_matches('0');

        if r.is_empty() {
            Ok(BigInt("0".to_string()))
        } else if r.chars().all(|c| c.is_ascii_digit()) {
            Ok(BigInt(format!("{}{}", if signed { "-" } else { "" }, r)))
        } else {
            Err(NotANumber)
        }
    }
}

macro_rules! impl_From {
    ($($t:ident),*) => {
        $(impl From<$t> for BigInt {
            fn from(value: $t) -> Self {
                BigInt(value.to_string())
            }
        })*
    }
}

impl_From!(u8, u16, u32, u64, u128, U256);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_value() {
        assert_eq!(BigInt::from_str("123").unwrap(), BigInt("123".to_string()));
        assert_eq!(
            BigInt::from_str("-123").unwrap(),
            BigInt("-123".to_string())
        );
        assert_eq!(
            BigInt::from_str("00233").unwrap(),
            BigInt("233".to_string())
        );
        assert_eq!(BigInt::from_str("0").unwrap(), BigInt("0".to_string()));
        assert_eq!(BigInt::from_str("-0").unwrap(), BigInt("0".to_string()));
        assert_eq!(BigInt::from_str("000").unwrap(), BigInt("0".to_string()));
        assert_eq!(BigInt::from_str("-000").unwrap(), BigInt("0".to_string()));

        assert!(BigInt::from_str("123a").is_err());
        assert!(BigInt::from_str("a123").is_err());
        assert!(BigInt::from_str("123-").is_err());
        assert!(BigInt::from_str(" 123").is_err());
    }

    #[test]
    fn from_primitives() {
        assert_eq!(BigInt::from(123u8), BigInt("123".to_string()));

        assert_eq!(BigInt::from(12_345u16), BigInt("12345".to_string()));

        assert_eq!(BigInt::from(123_456u32), BigInt("123456".to_string()));

        assert_eq!(
            BigInt::from(12_345_678_901u64),
            BigInt("12345678901".to_string()),
        );

        assert_eq!(
            BigInt::from(123_456_789_012_345_678_901u128),
            BigInt("123456789012345678901".to_string()),
        );

        assert_eq!(
            BigInt::from(U256::from_str("12345678901234567890123456789012345678901").unwrap()),
            BigInt("12345678901234567890123456789012345678901".to_string())
        );
    }
}

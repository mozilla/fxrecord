// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::convert::{TryFrom, TryInto};
use std::error::Error;
use std::io;

use derive_more::Display;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::prelude::*;

/// The value of a pref.
///
/// Prefs are limited to booleans, numbers, and strings.
#[derive(Debug, Deserialize, PartialEq, Serialize)]
pub struct PrefValue(Value);

/// An error from attemtpting to coerce a `Value` into a
/// [`PrefValue`](struct.PrefValue.html).
#[derive(Debug, Display)]
pub enum PrefError {
    #[display(fmt = "Pref values cannot be null")]
    Null,

    #[display(fmt = "Pref values cannot be arrays")]
    Array,

    #[display(fmt = "Pref values cannot be objects")]
    Object,

    #[display(fmt = "Expected a colon (`:') while parsing a pref")]
    ExpectedColon,

    #[display(fmt = "Could not parse pref: {}", _0)]
    Json(serde_json::Error),
}

impl Error for PrefError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            PrefError::Json(ref e) => Some(e),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for PrefError {
    fn from(e: serde_json::Error) -> Self {
        PrefError::Json(e)
    }
}

impl TryFrom<Value> for PrefValue {
    type Error = PrefError;

    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Bool(..) | Value::Number(..) | Value::String(..) => Ok(PrefValue(v)),
            Value::Null => Err(PrefError::Null),
            Value::Array(..) => Err(PrefError::Array),
            Value::Object(..) => Err(PrefError::Object),
        }
    }
}

impl From<PrefValue> for Value {
    fn from(p: PrefValue) -> Value {
        p.0
    }
}

/// Write all the prefs from the iterator into the `w`.
pub async fn write_prefs<W, P>(w: &mut W, prefs: P) -> Result<(), io::Error>
where
    W: AsyncWrite + Unpin,
    P: Iterator<Item = (String, PrefValue)>,
{
    for (key, value) in prefs {
        let js_value: Value = value.into();

        w.write_all(&b"pref(\""[..]).await?;
        w.write_all(key.as_bytes()).await?;
        w.write_all(&b"\", "[..]).await?;
        w.write_all(js_value.to_string().as_bytes()).await?;
        w.write_all(&b");\n"[..]).await?;
    }

    Ok(())
}

/// Parse a preference of the form `name:value`, where value is a string, boolean, or number.
pub fn parse_pref(s: &str) -> Result<(String, PrefValue), PrefError> {
    if let Some(idx) = s.find(':') {
        let (key, rest) = s.split_at(idx);
        let str_value = &rest[1..];

        let js_value = serde_json::from_str::<Value>(str_value)?;

        Ok((key.into(), js_value.try_into()?))
    } else {
        Err(PrefError::ExpectedColon)
    }
}

#[cfg(test)]
mod test {
    use assert_matches::assert_matches;
    use indoc::indoc;
    use serde_json::Number;

    use super::*;

    #[test]
    fn test_parse_pref() {
        assert_matches!(
            parse_pref(r#"foo:"bar""#),
            Ok((key, value)) => {
                assert_eq!(key, "foo");
                assert_eq!(value, PrefValue(Value::String("bar".into())));
            }
        );

        assert_matches!(
            parse_pref(r#"foo:"\"bar\"""#),
            Ok((key, value)) => {
                assert_eq!(key, "foo");
                assert_eq!(value, PrefValue(Value::String(r#""bar""#.into())));
            }
        );
    }

    #[test]
    fn test_try_from() {
        assert_matches!(PrefValue::try_from(Value::Null), Err(PrefError::Null));
        assert_matches!(
            PrefValue::try_from(Value::Array(vec![])),
            Err(PrefError::Array)
        );
        assert_matches!(
            PrefValue::try_from(Value::Object(serde_json::map::Map::new())),
            Err(PrefError::Object)
        );

        assert_matches!(
            PrefValue::try_from(Value::String("hello, world".into())),
            Ok(PrefValue(Value::String(s))) => {
                assert_eq!(s, "hello, world");
            }
        );

        assert_matches!(
            PrefValue::try_from(Value::Number(serde_json::Number::from_f64(0f64).unwrap())),
            Ok(PrefValue(Value::Number(n))) => {
                assert_eq!(n.as_f64().unwrap(), 0f64);
            }
        );

        assert_matches!(
            PrefValue::try_from(Value::Bool(true)),
            Ok(PrefValue(Value::Bool(true)))
        );
    }

    #[tokio::test]
    async fn test_write_prefs() {
        let mut buf: Vec<u8> = vec![];

        write_prefs(
            &mut buf,
            vec![
                (
                    "foo".into(),
                    PrefValue(Value::String("hello, world".into())),
                ),
                (
                    "bar".into(),
                    PrefValue(Value::String(r#""hello, world""#.into())),
                ),
                ("baz".into(), PrefValue(Value::Bool(true))),
                ("qux".into(), PrefValue(Value::Bool(false))),
                (
                    "quux".into(),
                    PrefValue(Value::Number(Number::from_f64(0f64).unwrap())),
                ),
                ("corge".into(), PrefValue(Value::Number(1u64.into()))),
                (
                    "grault".into(),
                    PrefValue(Value::Number(Number::from(-1i64))),
                ),
            ]
            .into_iter(),
        )
        .await
        .unwrap();

        assert_eq!(
            std::str::from_utf8(&buf).unwrap(),
            indoc!(
                r#"pref("foo", "hello, world");
                pref("bar", "\"hello, world\"");
                pref("baz", true);
                pref("qux", false);
                pref("quux", 0.0);
                pref("corge", 1);
                pref("grault", -1);
                "#
            )
        );
    }
}

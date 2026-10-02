use crate::{Fault, MAX_BODY};
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::Value;
use std::fmt;

/// Reject duplicate keys before typed deserialization (Value alone would overwrite them).
struct Strict(Value);
impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(StrictVisitor)
    }
}
struct StrictVisitor;
impl<'de> Visitor<'de> for StrictVisitor {
    type Value = Strict;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("valid JSON without duplicate keys")
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Strict, E> {
        Ok(Strict(v.into()))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Strict, E> {
        Ok(Strict(v.into()))
    }
    fn visit_string<E: de::Error>(self, v: String) -> Result<Strict, E> {
        Ok(Strict(v.into()))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Strict, E> {
        Ok(Strict(v.into()))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Strict, E> {
        Ok(Strict(v.into()))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Strict, E> {
        serde_json::Number::from_f64(v)
            .map(|n| Strict(Value::Number(n)))
            .ok_or_else(|| E::custom("invalid number"))
    }
    fn visit_unit<E: de::Error>(self) -> Result<Strict, E> {
        Ok(Strict(Value::Null))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
        let mut out = Vec::new();
        while let Some(Strict(v)) = a.next_element()? {
            out.push(v);
        }
        Ok(Strict(Value::Array(out)))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
        let mut out = serde_json::Map::new();
        while let Some(k) = a.next_key::<String>()? {
            if out.contains_key(&k) {
                return Err(de::Error::custom("duplicate key"));
            }
            let Strict(v) = a.next_value()?;
            out.insert(k, v);
        }
        Ok(Strict(Value::Object(out)))
    }
}
pub fn strict_json<T: serde::de::DeserializeOwned>(input: &[u8]) -> Result<T, Fault> {
    if input.len() > MAX_BODY {
        return Err(Fault::PayloadTooLarge);
    }
    // Structural depth scan is string/escape aware and bounds allocation before parsing.
    let (mut depth, mut string, mut escaped) = (0usize, false, false);
    for &b in input {
        if string {
            if escaped {
                escaped = false
            } else if b == b'\\' {
                escaped = true
            } else if b == b'"' {
                string = false
            }
        } else {
            match b {
                b'"' => string = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > 16 {
                        return Err(Fault::InvalidRequest);
                    }
                }
                b'}' | b']' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    let Strict(v) = serde_json::from_slice(input).map_err(|_| Fault::InvalidRequest)?;
    serde_json::from_value(v).map_err(|_| Fault::InvalidRequest)
}

// structures and routines related to account balance information.
use serde::de::{self, Deserializer, Unexpected, Visitor};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Serialize, Deserialize, Debug)]
pub struct Balance {
    pub asset: String,
    #[serde(deserialize_with = "string_as_f64")]
    pub free: f64,
    #[serde(deserialize_with = "string_as_f64")]
    pub locked: f64,
}

#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
pub struct CrossMarginBalance {
    pub asset: String,
    pub borrowed: String,
    pub free: String,
    pub interest: String,
    pub locked: String,
    pub netAsset: String,
}

fn string_as_f64<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_str(F64Visitor)
}

struct F64Visitor;
impl<'de> Visitor<'de> for F64Visitor {
    type Value = f64;
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a string representation of a f64")
    }
    fn visit_str<E>(self, value: &str) -> Result<f64, E>
    where
        E: de::Error,
    {
        value.parse::<f64>().map_err(|_err| {
            E::invalid_value(Unexpected::Str(value), &"a string representation of a f64")
        })
    }
}

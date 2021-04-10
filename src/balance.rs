// structures and routines related to account balance information.
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Balance {
    pub asset: String,
    pub free: String,
    pub locked: String,
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

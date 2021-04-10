use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Price {
    pub symbol: String,
    pub price: String,
}

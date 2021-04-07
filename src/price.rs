use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Price {
    pub symbol: String,
    pub price: String, 
}

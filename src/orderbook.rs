use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BidAsk {
    pub price: String,
    pub qty: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[allow(non_snake_case)]
pub struct OrderBook {
    lastUpdateId: u64,
    pub bids: Vec<BidAsk>,
    pub asks: Vec<BidAsk>,
}

impl OrderBook {
    #[allow(dead_code)]
    pub fn get_bids(&self) -> &Vec<BidAsk> {
        &self.bids
    }

    #[allow(dead_code)]
    pub fn get_asks(&self) -> &Vec<BidAsk> {
        &self.asks
    }
}

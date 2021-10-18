// structures and routines related to account information.
use crate::balance;

use balance::{Balance, CrossMarginBalance};

use serde::{Deserialize, Serialize};

use std::collections::HashMap;

use serde_json;

#[derive(Serialize, Deserialize, Debug, Default)]
#[allow(non_snake_case)]
pub struct Account {
    pub makerCommission: u32,
    pub takerCommission: u32,
    pub buyerCommission: u32,
    pub sellerCommission: u32,
    pub canTrade: bool,
    pub canWithdraw: bool,
    pub canDeposit: bool,
    pub updateTime: u64,
    pub accountType: String,
    pub balances: Vec<Balance>,
    pub permissions: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
pub struct CrossMarginAccount {
    pub borrowEnabled: bool,
    pub marginLevel: String,
    pub totalAssetOfBtc: String,
    pub totalLiabilityOfBtc: String,
    pub totalNetAssetOfBtc: String,
    pub tradeEnabled: bool,
    pub transferEnabled: bool,
    pub userAssets: Vec<CrossMarginBalance>,
}

#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
pub struct IsolatedAsset {
    pub asset: String,
    pub borrowEnabled: bool,
    pub borrowed: String,
    pub free: String,
    pub interest: String,
    pub locked: String,
    pub netAsset: String,
    pub netAssetOfBtc: String,
    pub repayEnabled: bool,
    pub totalAsset: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
pub struct IsolatedAssetInfo {
    pub baseAsset: HashMap<String, serde_json::Value>,
    pub quoteAsset: HashMap<String, serde_json::Value>,
    pub symbol: String,
    pub isolatedCreated: bool,
    pub marginLevel: String,
    pub marginLevelStatus: String, // "EXCESSIVE", "NORMAL", "MARGIN_CALL", "PRE_LIQUIDATION", "FORCE_LIQUIDATION"
    pub marginRatio: String,
    pub indexPrice: String,
    pub liquidatePrice: String,
    pub liquidateRate: String,
    pub tradeEnabled: bool,
}

#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
pub struct IsolatedMarginAccount {
    pub assets: Vec<IsolatedAssetInfo>,
}

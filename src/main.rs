mod account;
mod balance;
mod binance;
mod candlestick;
mod config;
mod exchangeinfo;
mod ma;
mod margin;
mod order;
mod orderbook;
mod position;
mod price;
mod process_md;
mod trading;
mod tradingpair;
mod utils;

use log::debug;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config_file = "conf/ct.ini".to_string();
    let (global_config, exchange_config) = config::new(&config_file);
    utils::init_logging("/var/log/crypto-trader", &global_config.log_level);
    debug!(
        "loaded configuration {:#?} from {:#?}.",
        global_config, config_file
    );

    let strat_cfg = global_config.get_strategy();
    process_md::run_strategy(strat_cfg, &exchange_config);

    Ok(())
}

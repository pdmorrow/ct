mod binance;
mod config;
mod exchange;
mod strategy;
mod price;

use binance::Binance;
use exchange::Exchange;
use config::Config;
use flexi_logger::{detailed_format, Age, Cleanup, Criterion, Duplicate, Logger, Naming};
use log::info;

fn run_strategies(cfg: &Config, ex: &dyn exchange::Exchange) {
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Logger::with_str("info")
        .log_to_file()
        .directory("logs")
        .format(detailed_format)
        .duplicate_to_stdout(Duplicate::Info)
        .create_symlink("current.log")
        .rotate(
            Criterion::Age(Age::Day),
            Naming::Timestamps,
            Cleanup::KeepLogFiles(7),
        )
        .start()?;

    info!("starting up...");

    let config_file = "conf/ct.ini".to_string();
    let config = config::new(&config_file);
    info!("loaded configuration from {:?}.", config_file);

    let exchange_config = config.exchange;
    let bex: Binance = Exchange::new(Box::new(exchange_config));

    let conn = bex.test_connectivity();

    info!(
        "exchange {:?} connection test: {:?} ",
        bex.config.name, conn
    );

    if conn == true {
        // Run forever or until we get a signal to exit.
        run_strategies(&config, &bex);
    }
    

    Ok(())
}

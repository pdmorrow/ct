use hmac::{Hmac, Mac, NewMac};
use sha2::Sha256;

use std::collections::HashMap;

use flexi_logger::{
    colored_detailed_format, Age, Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming,
};

fn get_hmac(secret: &str, input: &str) -> String {
    type HmacSha256 = Hmac<Sha256>;

    let mut mac = HmacSha256::new_varkey(secret.as_bytes()).unwrap();
    mac.update(input.as_bytes());
    let hash_msg = mac.finalize().into_bytes();
    hex::encode(&hash_msg)
}

pub fn sign_query(secret: &str, query_params: &HashMap<&str, &str>) -> String {
    let queryparts: Vec<String> = query_params
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect();
    let querystr = queryparts.join("&");
    get_hmac(secret, &querystr)
}

pub fn decimal_places(input: &str) -> u8 {
    let trim_zeros = input.trim_end_matches("0");
    let whole_and_decimal: Vec<&str> = trim_zeros.split(".").collect();
    if whole_and_decimal.len() == 1 {
        0 as u8
    } else {
        whole_and_decimal[1].len() as u8
    }
}

pub fn init_logging(logdir: &str, logspec: &str) {
    Logger::try_with_str(logspec)
        .unwrap()
        .log_to_file(FileSpec::default().directory(logdir))
        .format(colored_detailed_format)
        .duplicate_to_stdout(Duplicate::Info)
        .create_symlink("current.log")
        .rotate(
            Criterion::Age(Age::Day),
            Naming::Timestamps,
            Cleanup::KeepLogFiles(7),
        )
        .start()
        .unwrap();
}

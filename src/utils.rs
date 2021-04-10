use hmac::{Hmac, Mac, NewMac};
use sha2::Sha256;

use std::collections::HashMap;

use flexi_logger::{detailed_format, Age, Cleanup, Criterion, Duplicate, Logger, Naming};

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

pub fn init_logging(logdir: &str, logspec: &str) {
    Logger::with_str(logspec)
        .log_to_file()
        .directory(logdir)
        .format(detailed_format)
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

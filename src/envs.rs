use std::{env, process};

pub fn get_env(e: &str) -> String {
    let value = match e {
        "DBPULSE_EVERY" => env::var(e).unwrap_or("30".into()),
        "DBPULSE_ENVIRONMENT" => env::var(e).unwrap_or("unknown".into()),
        "DSN" => env::var(e).unwrap_or_else(|e| {
            println!("could not find DSN: {}", e);
            process::exit(1);
        }),
        _ => "??".into(),
    };
    return value;
}

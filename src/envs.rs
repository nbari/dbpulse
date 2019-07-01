use std::{env, process};

pub fn get_env(e: &str) -> String {
    let value = match e {
        "DSN" => env::var(e).unwrap_or_else(|e| {
            println!("could not find DSN: {}", e);
            process::exit(1);
        }),
        "ENVIRONMENT" => env::var(e).unwrap_or("unknown".into()),
        "EVERY" => env::var(e).unwrap_or("30".into()),
        "THRESHOLD_HEALTHY" => env::var(e).unwrap_or("2".into()),
        "THRESHOLD_UNHEALTHY" => env::var(e).unwrap_or("2".into()),
        "SLACK_WEBHOOK_URL" => env::var(e).unwrap_or_else(|e| {
            println!("could not find {}: {}", "SLACK_WEBHOOK_URL", e);
            process::exit(1);
        }),
        _ => "??".into(),
    };
    return value;
}

use slack_hook::{Slack, PayloadBuilder};
use std::{env,process};

pub fn send_msg(msg: String) {
    let slack_url = env::var("SLACK_WEBHOOK_URL").unwrap_or_else(|e| {
        println!("could not find {}: {}", "SLACK_WEBHOOK_URL", e);
        process::exit(1);
    });

    let slack = Slack::new(&*slack_url).unwrap();

    let p = PayloadBuilder::new()
        .text(msg)
        .channel("#noisy-neighbours")
        .username("dbpulse")
        .icon_emoji(":warning:")
        .build()
        .unwrap();

    let res = slack.send(&p);

    match res {
        Ok(()) => println!("msg sent"),
        Err(x) => println!("ERR: {:?}",x)
    }
}

use crate::envs::get_env;
use slack_hook::{PayloadBuilder, Slack};

pub fn send_msg(msg: String) {
    let slack_url = get_env("SLACK_WEBHOOK_URL");
    let env = get_env("ENVIRONMENT");

    let slack = Slack::new(&*slack_url).unwrap();

    let p = PayloadBuilder::new()
        .text(format!("*{}*: {}", env, msg))
        .channel("#noisy-neighbours")
        .username("dbpulse")
        .icon_emoji(":warning:")
        .build()
        .unwrap();

    let res = slack.send(&p);

    match res {
        Ok(()) => println!("msg sent"),
        Err(x) => println!("ERR: {:?}", x),
    }
}

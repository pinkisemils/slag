#![feature(conservative_impl_trait)]
#![feature(splice)]
#![allow(unused_doc_comment)]

extern crate config;
extern crate futures;
extern crate irc as aatxe_irc;
extern crate serde;
extern crate simplelog;
extern crate slack;
extern crate slack_api;
extern crate slack_hook;
extern crate tokio_core;
extern crate tokio_pool;
extern crate tokio_timer;


#[macro_use]
extern crate clap;
#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate log;
#[macro_use]
extern crate serde_derive;


use std::thread;
use tokio_core::reactor::{Core, Handle};
use futures::sync::mpsc;


mod slack_client;
mod irc;
mod message;
mod errors;
mod cfg;
use slack_client::{SlackReceiver, SlackSender};
use errors::SlagErr;

fn logging_conf() -> simplelog::Config {
    use simplelog::*;

    Config {
        time: Some(Level::Error),
        target: Some(Level::Error),
        location: Some(Level::Error),
        level: Some(Level::Error),
        ..Default::default()
    }
}

fn init_logging(log_level: simplelog::LevelFilter) {
    use simplelog::*;
    TermLogger::init(log_level, logging_conf())
        .or_else(|_| SimpleLogger::init(log_level, logging_conf()))
        .expect("failed to initialize logger");
}


fn main() {
    let matches = clap_app!(slagw =>
        (version: "0.1")
        (author: "emilsp")
        (about: "Relays messages between IRC and Slack")
        (@arg CONFIG: -c --config +takes_value "Custom path for config file")
        (@arg debug: -d ... "Enable debug output")
    ).get_matches();


    let logging_level = if matches.is_present("debug") {
        simplelog::LevelFilter::Debug
    } else {
        simplelog::LevelFilter::Warn
    };
    let config_file = matches.value_of("CONFIG");

    init_logging(logging_level);
    // Same as before...
    let mut ev = Core::new().unwrap();

    let (irc_send, irc_receive) = mpsc::channel(1024);
    let (mut slack_send, slack_receive) = mpsc::channel(1024);
    let cfg = match get_config(config_file) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to load config: {:?}", e);
            return;
        }
    };

    let (mut irc_cfg, slack_cfg) = cfg.get_cfg();

    let (mut cli, mut slack_agent) = match load_slack_receiver(slack_cfg.clone(), irc_send) {
        Ok(slack) => slack,
        Err(e) => {
            error!("Failed to load slack: {}", e.description());
            return;
        }
    };

    let slack_client_secret = slack_cfg.secret.to_string();
    let slack_sender = match load_slack_sink(slack_receive, slack_cfg, &ev.handle()) {
        Ok(s) => s,
        Err(e) => {
            error!("failed to load slack sender: {}", e.description());
            return;
        }
    };
    slack_sender.process(&ev.handle());

    thread::spawn(move || loop {
        let res = cli.run(&mut slack_agent);
        if let Err(e) = res {
            error!("restarting the slack connection after error: {}", e);
        }
        match slack::RtmClient::login(&slack_client_secret) {
            Ok(new_cli) => cli = new_cli,
            Err(e) => error!("failed to reconnect to slack {}", e),
        }
    });

    // cranking the event loop
    info!("starting up the relay");
    match irc_cfg.run(&mut ev, irc_receive, &mut slack_send) {
        Ok(i) => i,
        Err(e) => {
            error!("Failed to run irc: {}", e.description());
            return;
        }
    };
    warn!("irc stopped");
}

fn load_slack_receiver(
    cfg: slack_client::SlackCfg,
    irc_stream: mpsc::Sender<message::SlackMsg>,
) -> Result<(slack::RtmClient, SlackReceiver), errors::SlagErr> {
    let cli = slack::RtmClient::login(&cfg.secret.clone())?;
    let slack_agent = SlackReceiver::new(cfg, irc_stream, &cli);
    Ok((cli, slack_agent))
}

fn load_slack_sink(
    slack_sink: mpsc::Receiver<message::SlackMsg>,
    cfg: slack_client::SlackCfg,
    handle: &Handle,
) -> Result<SlackSender, SlagErr> {
    SlackSender::new(slack_sink, cfg, handle)
}

fn get_config(path: Option<&str>) -> Result<cfg::Cfg, errors::SlagErr> {
    use config::Source;
    let mut c = config::Config::new();
    if let Some(p) = path {
        c.merge(config::File::with_name(&p))?;
    } else {
        // this is why rust is sucky
        // can't just do get_home_config().as_path().to_str().unwrap_or("") due to lifetime issues.
        // Nevermind the ugliness of the statement above as it is.
        let empty_home_conf = "";
        let home_conf = get_home_config();
        let home_conf = home_conf.as_path();
        config::File::with_name("config.yaml")
            .collect_to(&mut c.cache)
            .or_else(|_| {
                config::File::with_name(home_conf.to_str().unwrap_or(empty_home_conf))
                    .collect_to(&mut c.cache)
            })
            .or_else(|_| {
                config::File::with_name("/etc/slagw/config.yaml").collect_to(&mut c.cache)
            })?;
    };
    let cfg = c.try_into()?;
    Ok(cfg)
}

fn get_home_config() -> std::path::PathBuf {
    use std::env::home_dir;
    use std::path::PathBuf;
    let mut buf = home_dir().unwrap_or(PathBuf::new());
    buf.push(r".config/slagw/config.yaml");
    buf
}

#![feature(conservative_impl_trait)]
#![allow(unused_doc_comment)]

extern crate slack;
extern crate slack_api;
extern crate config;
extern crate serde;
extern crate tokio_irc_client;
extern crate futures;
extern crate pircolate;
extern crate tokio_core;
extern crate tokio_pool;
extern crate log4rs;
extern crate slack_hook;
extern crate irc as aatxe_irc;
extern crate actix;

#[macro_use]
extern crate log;
#[macro_use]
extern crate error_chain;
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

fn main() {

    log4rs::init_file("config/log4rs.yml", Default::default())
        .expect("Couldn't open logging config file");

    let mut ev = Core::new().unwrap();

    let (irc_send, irc_receive) = mpsc::channel(1024);
    let (slack_send, slack_receive) = mpsc::channel(1024);
    let cfg = match get_config() {
        Ok(c) => c,
        Err(e) => {
            println!("Failed to load config: {:?}", e);
            return;
        }
    };

    let  (irc_cfg, slack_cfg)  = cfg.get_cfg();

    let (mut cli, mut slack_agent) = match load_slack_receiver(slack_cfg.clone(), irc_send) {
        Ok(slack) => slack,
        Err(e) => {
            println!("Failed to load slack: {}", e.description());
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
    let handle = ev.handle();

    thread::spawn(move || {
        loop {
            let res = cli.run(&mut slack_agent);
            if let Err(e) = res {
                error!("restarting the slack connection after error: {}", e);
            }
            match slack::RtmClient::login(&slack_client_secret) {
                Ok(new_cli) => cli = new_cli,
                Err(e) => error!("failed to reconnect to slack {}", e),
            }
        }
    });

    // cranking the event loop
    info!("starting up the relay");
    loop {
        ev.turn(None)
    }

}

fn load_slack_receiver(cfg: slack_client::SlackCfg,
                       irc_stream: mpsc::Sender<message::SlackMsg>)
                       -> Result<(slack::RtmClient, SlackReceiver), errors::SlagErr> {
    let cli = slack::RtmClient::login(&cfg.secret.clone())?;
    let slack_agent = SlackReceiver::new(cfg, irc_stream, &cli);
    Ok((cli, slack_agent))
}

fn load_slack_sink(slack_sink: mpsc::Receiver<message::SlackMsg>,
                   cfg: slack_client::SlackCfg,
                   handle: &Handle)
                   -> Result<SlackSender, SlagErr> {
    SlackSender::new(slack_sink, cfg, handle)
}

fn get_config() -> Result<cfg::Cfg, errors::SlagErr> {
    let mut c = config::Config::new();
    c.merge(config::File::with_name("config.yaml"))?;
    let cfg = c.try_into()?;
    Ok(cfg)
}

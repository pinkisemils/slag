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

#[macro_use] extern crate log;
#[macro_use] extern crate error_chain;
#[macro_use] extern crate serde_derive;


use std::path::Path;
use std::collections::VecDeque;
use std::thread;
use std::time::Duration;
use std::io::BufReader;
use std::io::BufRead;
use std::fs::File;
use std::io::Read;
use std::thread::sleep;

use tokio_core::reactor::Core;

use futures::sync::mpsc;


mod slack_client;
mod irc;
mod message;
mod errors;
mod cfg;
use slack_client::SlackReceiver;
use message::Msg;

fn main() {

    log4rs::init_file("config/log4rs.yml", Default::default()).expect("Couldn't open logging config file");

    let mut ev = Core::new().unwrap();

    let (irc_send, irc_receive) = mpsc::channel(1024);
    let (slack_send, slack_receive) = mpsc::channel(1024);
    let cfg = match get_config() {
        Ok(c) => c,
        Err(e) => {
            println!("Failed to load config: {:?}", e);//.description());
            return;
        }
    };

    let cfg::Cfg{irc_cfg, slack_cfg} = cfg;

    let (mut cli, mut slack_agent) = match load_slack(slack_cfg, irc_send) {
        Ok(slack) => slack,
        Err(e) => {
            println!("Failed to load slack: {}", e.description());
            return;
        }
    };

    let irc_agent = match irc::IrcConn::from_cfg(irc_cfg, &mut ev, irc_receive, slack_send) {
        Ok(i) => i,
        Err(e) => {
            println!("Failed to load slack: {}", e.description());
            return;
        }
    };
    info!("waiting now");
    thread::spawn(move || cli.run(&mut slack_agent));
    ev.run(irc_agent.process());


}

fn load_slack(cfg: slack_client::SlackCfg, irc_stream: mpsc::Sender<Msg>) -> Result<(slack::RtmClient, SlackReceiver), errors::SlagErr> {
    let mut cli = slack::RtmClient::login(&cfg.secret.clone())?;
    let slack_agent = SlackReceiver::new(cfg, irc_stream);
    Ok((cli, slack_agent))
}

//fn load_irc(cfg: &config::Config) -> Result<irc::IrcConn, errors::SlagErr> {
//
//}

fn get_config() -> Result<cfg::Cfg, errors::SlagErr> {
    let mut c = config::Config::new();
    c.merge(config::File::with_name("config.yaml"))?;
    let cfg = c.try_into()?;
    Ok(cfg)
}


use slack_api;
use slack;
use slack::{Event, Message};
use slack::api::channels::{ListRequest, list as list_channels};
use message;
use std::fmt;

use errors::SlagErr;

use futures::sync::mpsc::{Receiver, Sender, SendError};
use futures::{Sink,Stream};

use message::{TransMsg, Msg, SlackMsg};
use std::collections::HashMap;
use std::collections::hash_map::Entry::{Occupied, Vacant};
use tokio_core::reactor;
use futures;
use slack_hook::{Slack,PayloadBuilder};

#[derive(Deserialize,Serialize,Clone)]
pub struct SlackCfg {
    pub secret: String,
    pub hook_url: String,
    pub channel_map: HashMap<String, String>,
}


pub struct SlackReceiver {
    irc_chan: Sender<message::Msg>,
    cfg: SlackCfg,
    nicks: HashMap<String, String>,
    // maps channel IDs to channel names
    channels: HashMap<String, String>,
}

fn unwrap_chan(chan: &slack_api::Channel) -> Option<(String, String)> {
    None

}

fn unwrap_user(usr: &slack_api::User) -> Option<(String, String)> {
    None
}

impl SlackReceiver {
    pub fn new(cfg: SlackCfg,
               irc_chan: Sender<message::Msg>,
               cli: &slack::RtmClient)
               -> SlackReceiver {
        let mut nicks = HashMap::new();
        let mut channels = HashMap::new();
        let resp = cli.start_response();
        resp
            .channels
            .as_ref()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(unwrap_chan)
            .for_each(|(id, name)| {
                channels.insert(id, name);
            });
        resp
            .users
            .as_ref()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(unwrap_user)
            .for_each(|(id, name)| {
                nicks.insert(id, name);
            });

        SlackReceiver {
            irc_chan: irc_chan,
            cfg: cfg,
            nicks: HashMap::new(),
            channels: HashMap::new(),
        }
    }

    fn send_msg(&mut self, msg: Msg) {
        match self.irc_chan.try_send(msg) {
            Ok(_) => (),
            Err(e) => warn!("Failed to send message to irc channel"),
        }
    }


    fn handle_event(&mut self, event: slack::Event, rtmClient: &slack::RtmClient) {
        match event {
            Event::Message(msg) => self.handle_msg(*msg, rtmClient),
            _ => (),
        }
    }

    fn handle_msg(&mut self, pMessage: slack::Message, rtmClient: &slack::RtmClient) {
        println!("received slack event - {:?}", pMessage)
    }
}


impl slack::EventHandler for SlackReceiver {
    fn on_event(&mut self, cli: &slack::RtmClient, event: slack::Event) {
        debug!("event: {:?}", event);
    }

    fn on_close(&mut self, client: &slack::RtmClient) {
        debug!("on_close");
    }

    fn on_connect(&mut self, client: &slack::RtmClient) {
        debug!("Just joined, available channels are:");
    }
}

pub struct SlackSender {
    sink: Receiver<SlackMsg>,
    cfg: SlackCfg,
    slack: Slack,
}

impl SlackSender {
    pub fn new (sink: Receiver<message::SlackMsg>, cfg: SlackCfg, handle: &reactor::Handle) ->  Result<SlackSender, SlagErr> {
        let slack = Slack::new(cfg.hook_url.as_str(), handle)?;
        Ok(SlackSender {
            sink: sink,
            cfg: cfg,
            slack: slack,
        })

    }

    pub fn process(self, handle: &reactor::Handle) {
        let SlackSender{sink, cfg, slack }= self;
        let work = sink.filter_map(move |m| {
            match m {
                SlackMsg::OutMsg(pmsg) => {
                    if let Ok(p) = PayloadBuilder::new()
                        .text(pmsg.msg.clone())
                        .channel("#general")
                        .username(pmsg.nick.clone())
                        .build() {
                            Some(slack.send(&p))
                        } else {
                            error!("failed to construct a slack message from {:?}", pmsg);
                            None
                        }
                }
                _ => None
            }
        }).then(|res| {
            if let Err(e) = res {
                error!("failed to send message to slack: {:?}", e);
            };
            Ok(())
        }).for_each(|_| Ok(()));
        handle.spawn(work);
    }
}

use slack;
use slack::{Event, Message};
use slack::api::channels::{ListRequest, list as list_channels};
use message;
use std::fmt;

use errors::SlagErr;

use futures::sync::mpsc::{Sender, SendError};
use futures::Sink;
use message::{TransMsg, Msg};
use std::collections::HashMap;
use std::collections::hash_map::Entry::{Occupied, Vacant};

#[derive(Deserialize,Serialize)]
pub struct SlackCfg {
    pub secret: String,
}


pub struct SlackReceiver {
    irc_chan: Sender<message::Msg>,
    cfg: SlackCfg,
    nicks: HashMap<String, String>,
    // maps channel IDs to channel names
    channels: HashMap<String, String>,
}

impl SlackReceiver {
    pub fn new(cfg: SlackCfg, irc_chan: Sender<message::Msg>) -> SlackReceiver {
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
        //    match client.list_channels() {
        //        Ok(chans) => {
        //            for chan in chans {
        //                debug!("chan: {:?}", chan);
        //            }
        //        }
        //        Err(e) => debug!("Failed to list channels because of error {}", e),
        //    }
    }
}

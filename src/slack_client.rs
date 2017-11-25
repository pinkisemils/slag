use slack_api;
use slack;
use slack::{Event, Message};
use slack::api::channels::{ListRequest, list as list_channels};
use message;

use errors::SlagErr;

use futures::sync::mpsc::{Receiver, Sender, SendError};
use futures::{Sink, Stream};

use message::{TransMsg, PrivMsg, SlackMsg};
use std::collections::HashMap;
use std::collections::hash_map::Entry::{Occupied, Vacant};
use tokio_core::reactor;
use futures;
use futures::Future;
use slack_hook::{Slack, PayloadBuilder, Payload as SlackPayload};

#[derive(Deserialize,Serialize,Clone)]
pub struct SlackCfg {
    pub secret: String,
    pub hook_url: String,
    pub channels: HashMap<String, String>,
}


pub struct SlackReceiver {
    irc_chan: Sender<message::PrivMsg>,
    cfg: SlackCfg,
    nicks: HashMap<String, String>,
    // maps channel IDs to channel names
    channels: HashMap<String, String>,
}

fn unwrap_chan_mapping(chan: &slack_api::Channel) -> Option<(String, String)> {
    let id = chan.id.as_ref()?.clone();
    let name = chan.name.as_ref()?.clone();
    Some((id, name))

}

fn unwrap_user_mapping(usr: &slack_api::User) -> Option<(String, String)> {
    let id = usr.id.as_ref()?.clone();
    let name = usr.name.as_ref()?.clone();
    Some((id, name))
}

impl SlackReceiver {
    pub fn new(cfg: SlackCfg,
               irc_chan: Sender<message::PrivMsg>,
               cli: &slack::RtmClient)
               -> SlackReceiver {
        let mut nicks = HashMap::new();
        let mut channels = HashMap::new();
        let resp = cli.start_response();
        resp.channels
            .as_ref()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(unwrap_chan_mapping)
            .for_each(|(id, name)| {
                channels.insert(id, name);
            });
        resp.users
            .as_ref()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(unwrap_user_mapping)
            .for_each(|(id, name)| {
                nicks.insert(id, name);
            });

        SlackReceiver {
            irc_chan: irc_chan,
            cfg: cfg,
            nicks: nicks,
            channels: channels,
        }
    }

    fn send_irc_msg(&mut self, msg: PrivMsg) {
        if let Err(e) = self.irc_chan.try_send(msg) {
            error!("Failed to send message to irc channel - {:?}", e);
        };
    }


    fn handle_event(&mut self, event: slack::Event, rtm_client: &slack::RtmClient) {
        match event {
            Event::Message(msg) => self.handle_msg(*msg, rtm_client),
            _ => (),
        }
    }

    fn handle_msg(&mut self, pMessage: slack::Message, rtm_client: &slack::RtmClient) {
        self.slack_msg_to_privmsg(pMessage).map(|m| {
            self.send_irc_msg(m);
        });
    }

    fn slack_msg_to_privmsg(&mut self, s_msg: slack::Message) -> Option<message::PrivMsg> {
        match s_msg {
            slack::Message::Standard(m) => self.std_msg_to_priv(m),
            _ => None,
        }
    }

    fn std_msg_to_priv(&mut self, std_msg: slack_api::MessageStandard) -> Option<message::PrivMsg> {
        let nick = std_msg.user
            .map(|u| self.nicks.get(&u))??;
        let slack_chan = std_msg.channel
            .map(|c| self.channels.get(&c))??;
        let chan = self.cfg.channels.get(slack_chan)?;
        let text = std_msg.text?;

        Some(PrivMsg {
            nick: nick.clone(),
            chan: chan.clone(),
            msg: text,
        })
    }
}




impl slack::EventHandler for SlackReceiver {
    fn on_event(&mut self, cli: &slack::RtmClient, event: slack::Event) {
        self.handle_event(event, cli);
    }

    fn on_close(&mut self, _: &slack::RtmClient) {
        debug!("on_close");
    }

    fn on_connect(&mut self, _: &slack::RtmClient) {
        debug!("Just joined, available channels are:");
    }
}

pub struct SlackSender {
    sink: Receiver<SlackMsg>,
    cfg: SlackCfg,
    slack: Slack,
}

impl SlackSender {
    pub fn new(sink: Receiver<message::SlackMsg>,
               cfg: SlackCfg,
               handle: &reactor::Handle)
               -> Result<SlackSender, SlagErr> {
        let slack = Slack::new(cfg.hook_url.as_str(), handle)?;
        Ok(SlackSender {
            sink: sink,
            cfg: cfg,
            slack: slack,
        })

    }

    pub fn process(self, handle: &reactor::Handle) {
        let SlackSender { sink, cfg, slack } = self;
        let work = sink.map_err(|_| ())
            .filter_map(move |m| {
                println!("building slack message for: {:?}", m);
                match m {
                    SlackMsg::OutMsg(pmsg) => {
                        let msg = try_slack_msg_from_priv(pmsg)?;
                        Some(slack.send(&msg).map_err(|e| {
                            error!("failed to send to slack: {}", e);
                            ()
                        }))
                    }
                    _ => None,
                }
            })
            .buffered(1024)
            .for_each(|_| Ok(()));
        handle.spawn(work);
    }
}

fn try_slack_msg_from_priv(pmsg: PrivMsg) -> Option<SlackPayload> {
    if let Ok(p) = PayloadBuilder::new()
        .text(pmsg.msg.clone())
        .channel("#general")
        .username(pmsg.nick.clone())
        .icon_emoji(":winkwink:")
        .build() {
        Some(p)
    } else {
        None
    }
}

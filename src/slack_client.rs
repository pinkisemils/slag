use slack_api;
use slack;
use slack::Event;
use message;

use errors::SlagErr;

use futures::sync::mpsc::{Receiver, Sender};
use futures::Stream;
use futures::Future;

use tokio_core::reactor;

use slack_hook::{Slack, PayloadBuilder, Payload as SlackPayload};

use std::collections::HashMap;

use message::{PrivMsg, SlackMsg};

#[derive(Deserialize,Serialize,Clone, Debug)]
pub struct SlackCfg {
    pub secret: String,
    pub hook_url: String,
    pub channels: HashMap<String, String>,
}


pub struct SlackReceiver {
    irc_chan: Sender<message::SlackMsg>,
    cfg: SlackCfg,
    slack_nick_mappings: HashMap<String, String>,
    slack_channel_mappings: HashMap<String, String>,
    first_msg: bool,
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
               irc_chan: Sender<message::SlackMsg>,
               cli: &slack::RtmClient)
               -> SlackReceiver {
        let resp = cli.start_response();
        let channels = resp.channels
            .as_ref()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(unwrap_chan_mapping)
            .collect();
        let nicks = resp.users
            .as_ref()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(unwrap_user_mapping)
            .collect();

        SlackReceiver {
            irc_chan: irc_chan,
            cfg: cfg,
            slack_nick_mappings: nicks,
            slack_channel_mappings: channels,
            first_msg: true,
        }
    }

    fn send_irc_msg(&mut self, msg: SlackMsg) {
        if let Err(e) = self.irc_chan.try_send(msg) {
            error!("Failed to send message to irc channel - {:?}", e);
        };
    }


    fn handle_event(&mut self, event: slack::Event) {
        match event {
            Event::Message(msg) => self.handle_msg(*msg),
            _ => (),
        }
    }

    fn handle_msg(&mut self, slack_msg: slack::Message) {
        self.slack_msg_to_privmsg(slack_msg).map(|m| {
            self.send_irc_msg(SlackMsg::OutMsg(m));
        });
    }

    fn slack_msg_to_privmsg(&mut self, s_msg: slack::Message) -> Option<message::PrivMsg> {
        if self.first_msg {
            self.first_msg = false;
            return None;
        }
        match s_msg {
            slack::Message::Standard(m) => self.std_msg_to_priv(m),
            _ => None,
        }
    }

    fn notify_of_disconnect(&mut self) {
        let status_messages: Vec<PrivMsg> = self.cfg
            .channels
            .keys()
            .map(|chan| {
                PrivMsg {
                    chan: chan.clone(),
                    msg: "! DISCONNECTED FROM SLACK !".to_owned(),
                    nick: "".to_string(),
                }
            })
            .collect();

        for m in status_messages.into_iter() {
            self.send_irc_msg(SlackMsg::StatusMsg(m));
        }
    }

    fn std_msg_to_priv(&mut self, std_msg: slack_api::MessageStandard) -> Option<message::PrivMsg> {
        let nick = std_msg.user
            .map(|u| self.slack_nick_mappings.get(&u))??;
        let slack_chan = std_msg.channel
            .map(|c| self.slack_channel_mappings.get(&c))??;
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
    fn on_event(&mut self, _: &slack::RtmClient, event: slack::Event) {
        self.handle_event(event);
    }

    fn on_close(&mut self, _: &slack::RtmClient) {
        info!("disconnected from slack");
        self.notify_of_disconnect();
    }

    fn on_connect(&mut self, _: &slack::RtmClient) {
        info!("joined slack");
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
        let SlackCfg { channels, hook_url, secret } = cfg;
        let inverted_chan_map = channels.into_iter()
            .map(|(slack_chan, irc_chan)| (irc_chan, slack_chan))
            .collect();
        let cfg = SlackCfg {
            channels: inverted_chan_map,
            hook_url: hook_url,
            secret: secret,
        };
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
                match m {
                    SlackMsg::OutMsg(pmsg) => {
                        let msg = SlackSender::try_slack_msg_from_priv(&cfg, pmsg)?;
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

    fn try_slack_msg_from_priv(cfg: &SlackCfg, pmsg: PrivMsg) -> Option<SlackPayload> {
        let out_chan = cfg.channels.get(&pmsg.chan)?;
        if let Ok(msg_payload) = PayloadBuilder::new()
            .text(pmsg.msg.clone())

            .channel(out_chan.clone())
            .username(pmsg.nick.clone())
            .icon_emoji(":winkwink:")
            .build() {
            Some(msg_payload)
        } else {
            None
        }
    }
}

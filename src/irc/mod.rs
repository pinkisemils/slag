use std::collections::{HashMap, HashSet};
use std::net;
use std::default::Default;
use std::error::Error;

use tokio_core::reactor;

use futures::{future, stream, Future, IntoFuture, Sink, Stream};
use futures::sync::mpsc;


use pircolate::message::client as client_msg;
use pircolate::message::Message as irc_msg;
use pircolate::command as irc_cmd;
use pircolate::message::client::priv_msg;


//use tokio_irc_client::Client;
use tokio_irc_client::error::Error as IrcErr;
use tokio_irc_client::error::ErrorKind as IrcErrKind;

use message::{PrivMsg, SlackMsg};
use errors::{SlagErr, SlagErrKind};

use aatxe_irc::client::data::Config as AatxeConfig;
use aatxe_irc::client::reactor::IrcReactor;
use aatxe_irc::client::{Client, IrcClient, PackedIrcClient};
use aatxe_irc::proto::command::Command as AatxeCmd;
use aatxe_irc::proto::message::Message as AatxeMsg;
use aatxe_irc::client::ext::ClientExt;

#[derive(Deserialize, Serialize)]
pub struct IrcChan {
    ignored_nicks: HashSet<String>,
    target_chan: String,
}

#[derive(Deserialize, Serialize)]
pub struct IrcCfg {
    host: String,
    port: u16,
    nick: String,
    user: String,
    #[serde(skip)]
    pub channels: HashMap<String, String>,
}

impl IrcCfg {
    fn conn_from_cfg(&self) -> AatxeConfig {
        AatxeConfig {
            nickname: Some(self.nick.clone()),
            server: Some(self.host.clone()),
            port: Some(self.port),
            ..Default::default()
        }
    }

    pub fn run_once(
        &mut self,
        core: &mut reactor::Core,
        in_stream: mpsc::Receiver<SlackMsg>,
        mut slack_chan: &mut mpsc::Sender<SlackMsg>,
    ) -> Result<(), SlagErr> {
        let cfg = self.conn_from_cfg();
        let future = IrcClient::new_future(core.handle(), &cfg).unwrap();
        // immediate connection errors (like no internet) will turn up here...
        let PackedIrcClient(client, future) = core.run(future).unwrap();
        let sender = client.clone();
        // runtime errors (like disconnections and so forth) will turn up here...
        let slack_sender = consume_sender(in_stream, sender);


        let work = client
            .stream()
            .filter_map(handle_irc_msg)
            .for_each(|msg| {
                try_send_to_slack(&mut slack_chan, msg);
                Ok(())
            })
            .join(future);
        core.run(work).unwrap();

        // let res = core.prepare_client_and_connect(&cfg)
        //         .and_then(|cl| cl.identify().and(Ok(cl)))
        //         .and_then(|cl| {
        //             let sender = cl.clone();
        //             let sink = in_stream
        //                 .filter_map(handle_slack_msg)
        //                 .for_each(|m| {
        //                     sender.send(m);
        //                     Ok(())
        //                 });
        //             core.inner_handle().spawn(sink);
        //             core.register_client_with_handler(cl, |_, message| {

        //             });

        //             Ok(())
        //         });
        Ok(())
    }
}




fn consume_sender(
    output_stream: mpsc::Receiver<SlackMsg>,
    sender: IrcClient,
) -> Box<Future<Item = mpsc::Receiver<SlackMsg>, Error = SlagErr> + Send> {
    let next = output_stream.into_future();
    next.then(|res| {
        let (msg, stream) = res.unwrap();
        let msg = match msg {
            Some(m) => m,
            None => return Err(SlagErr::from(SlagErrKind::SlackChanDown)),
        };

        let msg = match handle_slack_msg(msg) {
            None => {
                return Ok(consume_sender(stream, sender).boxed());
            }
            Some(m) => m,
        };

        match sender.send(msg) {
            Ok(_) => Ok(consume_sender(stream, sender).boxed()),
            Err(e) => {
                error!("encountered error sending to irc: {:?}", e);
                Ok(future::ok(stream).boxed())
            }
        }
    }).flatten()
        .boxed()
}

fn resolve_consumption(
    stream: mpsc::Receiver<SlackMsg>,
) -> impl Future<Item = mpsc::Receiver<SlackMsg>, Error = SlagErr> {
    future::ok(stream)
}

// fn handle_msg_send(r: Result<String, ()>) {
//     match r {
//         Err(e) => error!("ayy lmau: {}", e),
//         _ => (),
//     }
// }
//

fn try_send_to_slack(chan: &mut mpsc::Sender<SlackMsg>, slack_msg: SlackMsg) {
    if let Err(e) = chan.try_send(slack_msg) {
        let desc = e.description().to_string();
        let dropped_message = e.into_inner();
        error!(
            "dropped message '{:?}' when sending to slack: {}",
            dropped_message,
            desc
        );
    }
}


fn handle_irc_msg(irc_msg: AatxeMsg) -> Option<SlackMsg> {
    let nick = irc_msg.source_nickname()?.to_string();
    let cmd = irc_msg.command;
    match cmd {
        AatxeCmd::PRIVMSG(target, msg) => Some(SlackMsg::OutMsg(PrivMsg {
            chan: target,
            msg: msg,
            nick: nick,
        })),
        _ => None,
    }
}

fn handle_slack_msg(slack_msg: SlackMsg) -> Option<AatxeCmd> {
    match slack_msg {
        SlackMsg::OutMsg(m) => try_format_out_msg(m),
        SlackMsg::StatusMsg(m) => try_format_status_msg(m),
    }
}

fn try_format_out_msg(m: PrivMsg) -> Option<AatxeCmd> {
    Some(AatxeCmd::PRIVMSG(
        m.chan,
        format!("[{}]: {}", m.nick, m.msg),
    ))
}

fn try_format_status_msg(m: PrivMsg) -> Option<AatxeCmd> {
    Some(AatxeCmd::PRIVMSG(m.chan, m.msg))
}



fn try_priv_msg(irc_msg: &irc_msg) -> Option<PrivMsg> {
    if let Some(pmsg) = irc_msg.command::<irc_cmd::PrivMsg>() {
        let irc_cmd::PrivMsg(chan, msg) = pmsg;
        if let Some((nick, _, _)) = irc_msg.prefix() {
            Some(PrivMsg {
                nick: nick.to_string(),
                chan: chan.to_string(),
                msg: msg.to_string(),
            })
        } else {
            None
        }
    } else {
        None
    }
}

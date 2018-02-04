use std::collections::{HashMap, HashSet};
use std::net;
use std::default::Default;
use std::error::Error;

use tokio_core::reactor;

use futures::{future, stream, Future, IntoFuture, Sink, Stream};
use futures::sync::mpsc;
use futures::sync::oneshot::Canceled;


use pircolate::message::client as client_msg;
use pircolate::message::Message as irc_msg;
use pircolate::command as irc_cmd;
use pircolate::message::client::priv_msg;


//use tokio_irc_client::Client;
use tokio_irc_client::error::Error as IrcErr;
use tokio_irc_client::error::ErrorKind as IrcErrKind;

use message::{PrivMsg, SlackMsg};
use errors::{SlagErr, SlagErrKind};

use aatxe_irc;
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

#[derive(Debug)]
enum IrcOutMsg {
    FromSlack(SlackMsg),
    Shutdown,
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

    fn run_once(
        &mut self,
        core: &mut Core,
        shutdown_chan: mpsc::Sender<IrcOutMsg>,
        in_stream: mpsc::Receiver<IrcOutMsg>,
        mut slack_chan: &mut mpsc::Sender<SlackMsg>,
    ) -> Result<mpsc::Receiver<IrcOutMsg>, SlagErr> {
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
            .then(|_| shutdown_chan.send(IrcOutMsg::Shutdown))
            .map_err(|_| aatxe_irc::error::IrcError::OneShotCanceled(Canceled {}))
            .map_err(|e| SlagErrKind::AatxeErr(e).into())
            .join3(
                future.map_err(|e| SlagErrKind::AatxeErr(e).into()),
                slack_sender,
            );

        // Sink error takes precedence - if the slack sink isn't there anymore, the loop has to be
        // shut down. Otherwise, return irc_err.
        match core.run(work) {
            Err(e) => Err(e),
            Ok((_, _, sink_chan)) => Ok(sink_chan),
        }
    }
}




fn consume_sender(
    output_stream: mpsc::Receiver<IrcOutMsg>,
    sender: IrcClient,
) -> Box<Future<Item = mpsc::Receiver<IrcOutMsg>, Error = SlagErr> + Send> {
    let next = output_stream.into_future();
    next.then(|res| {
        let (msg, stream) = res.unwrap();
        let msg = match msg {
            Some(m) => m,
            None => return Err(SlagErr::from(SlagErrKind::SlackChanDown)),
        };
        let msg = match msg {
            IrcOutMsg::Shutdown => return Ok(future::ok(stream).boxed()),
            IrcOutMsg::FromSlack(m) => m,
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

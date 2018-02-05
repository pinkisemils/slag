use std::collections::{HashMap, HashSet};
use std::net;
use std::default::Default;
use std::error::Error;

use tokio_core::reactor;

use futures::{future, Future, Sink, Stream};
use futures::sync::mpsc;
use futures::sync::oneshot::Canceled;
use futures::sync::oneshot;


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
    #[serde(skip)] pub channels: HashMap<String, String>,
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
            channels: Some(self.channels.keys().cloned().collect()),
            use_ssl: Some(false),
            ..Default::default()
        }
    }

    fn run_once(
        &mut self,
        core: &mut reactor::Core,
        shutdown_chan: mpsc::Sender<IrcOutMsg>,
        in_stream: mpsc::Receiver<IrcOutMsg>,
        mut slack_chan: &mut mpsc::Sender<SlackMsg>,
    ) -> Result<mpsc::Receiver<IrcOutMsg>, SlagErr> {
        let cfg = self.conn_from_cfg();
        let future = IrcClient::new_future(core.handle(), &cfg).unwrap();
        // immediate connection errors (like no internet) will turn up here...
        let PackedIrcClient(client, future) = core.run(future).unwrap();
        let sender = client.clone();
        let (stop_sender, recv_slack_stream) = oneshot::channel();
        // runtime errors (like disconnections and so forth) will turn up here...
        let slack_sender = consume_sender(in_stream, stop_sender, sender);

        core.handle().spawn(slack_sender);
        let res = client.identify();
        info!("result - {:?}", res);



        let work = client
            .stream()
            .filter_map(handle_irc_msg)
            .for_each(|msg| {
                try_send_to_slack(&mut slack_chan, msg);
                Ok(())
            })
            .then(|res| {
                info!("stopped sending because: {:?}", res);
                shutdown_chan.send(IrcOutMsg::Shutdown)
            })
            .map_err(|_| aatxe_irc::error::IrcError::OneShotCanceled(Canceled {}))
            .join(future);

        // Sink error takes precedence - if the slack sink isn't there anymore, the loop has to be
        // shut down. Otherwise, return irc_err.
        info!("trying to irc");
        let result = core.run(work);
        let slack_stream = match core.run(recv_slack_stream)
            .map_err(|_| SlagErr::from(SlagErrKind::SlackChanDown))?
        {
            Ok(s) => s,
            Err(e) => return Err(e),
        };

        // handle result better to return a proper error if IRC is dead
        info!("stopped sending/receiveing because: {:?}", result);
        return Ok(slack_stream);
    }


    pub fn run(
        &mut self,
        core: &mut reactor::Core,
        in_stream: mpsc::Receiver<SlackMsg>,
        slack_chan: &mut mpsc::Sender<SlackMsg>,
    ) -> Result<(), SlagErr> {
        let (sink_in, mut sink_out) = mpsc::channel(32);
        let slack_msgs = in_stream.map(|m| IrcOutMsg::FromSlack(m)).map_err(|_| ());

        let slack_pipe = sink_in
            .clone()
            .sink_map_err(|_| ())
            .send_all(slack_msgs)
            .then(|_| Ok(()));
        core.handle().spawn(slack_pipe);

        loop {
            match self.run_once(core, sink_in.clone(), sink_out, slack_chan) {
                Ok(slack_msg_chan) => {
                    sink_out = slack_msg_chan;
                    info!("trying to irc again");
                    continue;
                }
                Err(e) => {
                    info!("stopping because: {:?}", e);
                    return Err(e);
                }
            };
        }
    }
}




fn consume_sender(
    output_stream: mpsc::Receiver<IrcOutMsg>,
    out_chan: oneshot::Sender<Result<mpsc::Receiver<IrcOutMsg>, SlagErr>>,
    sender: IrcClient,
) -> Box<Future<Item = (), Error = ()> + Send> {
    let next = output_stream.into_future();
    next.then(|res| {
        let (msg, stream) = res.unwrap();
        info!("getting real tired of this shit: {:?}", msg);
        let msg = match msg {
            Some(m) => m,
            None => {
                out_chan
                    .send(Err(SlagErr::from(SlagErrKind::SlackChanDown)))
                    .map_err(|_| ());
                return future::ok(()).boxed();
            }
        };
        let msg = match msg {
            IrcOutMsg::Shutdown => {
                out_chan.send(Ok(stream));
                return future::ok(()).boxed();
            }
            IrcOutMsg::FromSlack(m) => m,
        };

        let msg = match handle_slack_msg(msg) {
            None => {
                return consume_sender(stream, out_chan, sender);
            }
            Some(m) => m,
        };

        match sender.send(msg) {
            Ok(_) => consume_sender(stream, out_chan, sender),
            Err(e) => {
                error!("encountered error sending to irc: {:?}", e);
                out_chan.send(Ok(stream));

                return future::ok(()).boxed();
            }
        }
    }).boxed()
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

use std::collections::{HashSet, HashMap};
use std::marker::Sized;
use std::path::Path;
use std::time;
use std::thread;
use std::net;
use std::io::Error as IoErr;

use tokio_core::reactor::Core;
use tokio_core::reactor::Handle as ReactorHandle;

use futures::{Future, Sink, Stream, stream};
use futures::sync::mpsc;


use config::Config;


use pircolate::message::client as client_msg;
use pircolate::message::Message as irc_msg;
use pircolate::command as irc_cmd;
use pircolate::message::client::priv_msg;


use tokio_irc_client::Client;
use tokio_irc_client::client::IrcTransport;
use tokio_irc_client::error as irc_err;

use tokio_core::net::TcpStream;

use message::{Msg, PrivMsg, SlackMsg};
use errors::{SlagErrKind, SlagErr, SlagResult};


pub struct TransMsg {
    pub chan: String,
    pub text: String,
}

#[derive(Deserialize,Serialize)]
pub struct IrcChan {
    ignored_nicks: HashSet<String>,
    target_chan: String,
}

#[derive(Deserialize,Serialize)]
pub struct IrcCfg {
    address: net::SocketAddr,
    nick: String,
    user: String,
    channels: HashMap<String, IrcChan>,
}

pub struct IrcConn {
    cfg: IrcCfg,
    handle: ReactorHandle,
    slack_sink: mpsc::Sender<SlackMsg>,
    sink: stream::SplitSink<IrcTransport<TcpStream>>,
}

impl IrcConn {
    pub fn from_cfg(cfg: IrcCfg,
                    core: &mut Core,
                    slack_chan: mpsc::Sender<SlackMsg>)
                    -> Result<IrcConn, SlagErr> {

        let mut connect_seq = vec![
            client_msg::nick(&cfg.nick),
            client_msg::user(&cfg.user, "IRC to Slack bridge"),
        ];
        connect_seq.extend(cfg.channels
            .iter()
            .map(|(chan_name, _)| client_msg::join(&chan_name, None)));

        let client = Client::new(cfg.address)
            .connect(&core.handle())
            .and_then(|c| c.send_all(stream::iter(connect_seq)))
            .and_then(|(c, _)| Ok(c.split()));
        let (irc_tx, irc_rx) = core.run(client)?;

        let incoming_messages = irc_rx.for_each(|message| {
                match try_priv_msg(&message) {
                    Some(pmsg) => {
                        info!("received real msg {:?}", pmsg);
                    }
                    None => info!("Received something {:?}", message),
                };
                Ok(())
            })
            .map_err(|_| ());
        core.handle().spawn(incoming_messages);


        Ok(IrcConn {
            cfg: cfg,
            handle: core.handle(),
            sink: irc_tx,
            slack_sink: slack_chan,
        })
    }

    pub fn process(conn: IrcConn,
                   in_stream: mpsc::Receiver<Msg>)
                   -> Box<Future<Item = (), Error = SlagErr>> {
        let slack_sink = conn.slack_sink.clone();
        let work = in_stream.for_each(move |msg| {
                match msg {
                    Msg::IrcInMsg(pmsg) => {
                        slack_sink.send(SlackMsg::OutMsg(pmsg));
                    }
                    _ => (),
                    Msg::IrcOutMsg(pmsg) => {
                        if let Ok(m) = priv_msg(&pmsg.chan,
                                                &format!("[{}]: {}", pmsg.nick, pmsg.msg)) {
                            conn.sink.send(m);
                        }
                    }
                };
                return Ok(());
            })
            .map_err(|_| SlagErr::from(SlagErrKind::InvalidErr("ayy lmau".to_string())));
        Box::new(work)
    }
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

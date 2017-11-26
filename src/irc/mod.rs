use std::collections::{HashSet, HashMap};
use std::net;

use tokio_core::reactor::Core;

use futures::{Future, Sink, Stream, stream};
use futures::sync::mpsc;


use pircolate::message::client as client_msg;
use pircolate::message::Message as irc_msg;
use pircolate::command as irc_cmd;
use pircolate::message::client::priv_msg;


use tokio_irc_client::Client;
use tokio_irc_client::error::Error as IrcErr;
use tokio_irc_client::error::ErrorKind as IrcErrKind;

use message::{PrivMsg, SlackMsg};
use errors::SlagErr;

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

pub fn init_irc(cfg: IrcCfg,
                core: &mut Core,
                in_stream: mpsc::Receiver<SlackMsg>,
                slack_chan: mpsc::Sender<SlackMsg>)
                -> Result<(), SlagErr> {

    let mut connect_seq = vec![
            client_msg::nick(&cfg.nick),
            client_msg::user(&cfg.user, "IRC to Slack bridge"),
        ];
    connect_seq.extend(cfg.channels
        .iter()
        .map(|(chan_name, _)| client_msg::join(&chan_name, None)));

    let client = Client::new(cfg.address)
        .connect(&core.handle())
        .and_then(|c| c.send_all(stream::iter_result(connect_seq)))
        .and_then(|(c, _)| Ok(c.split()));
    let (irc_tx, irc_rx) = core.run(client)?;
    info!("conencted to IRC");

    let incoming_messages = irc_rx.filter_map(|message| {
            match try_priv_msg(&message) {
                Some(pmsg) => Some(SlackMsg::OutMsg(pmsg)),
                None => None,
            }
        })
        .map_err(|e| {
            info!("irc connection is shut down: {}", e);
            ()
        })
        .forward(slack_chan.sink_map_err(|_| ()))
        .map(|_| ());

    core.handle().spawn(incoming_messages);

    let outgoing_messages = in_stream.filter_map(handle_slack_msg)
        .map_err(|_| IrcErr::from(IrcErrKind::Msg("message sender depleted".into())))
        .forward(irc_tx)
        .map(|_| ())
        .map_err(|e| {
            info!("stopping sending messages to irc because: {}", e);
            ()
        });
    core.handle().spawn(outgoing_messages);


    Ok(())
}

fn handle_slack_msg(slack_msg: SlackMsg) -> Option<irc_msg> {
    match slack_msg {
        SlackMsg::OutMsg(m) => try_format_out_msg(m),
        SlackMsg::StatusMsg(m) => try_format_status_msg(m),
    }
}


fn try_format_out_msg(m: PrivMsg) -> Option<irc_msg> {
    match priv_msg(&m.chan, &format!("[{}]: {}", m.nick, m.msg)) {
        Ok(m) => Some(m),
        Err(e) => {
            error!("failed to format a slack message into a real privmsg {:?}, because {}",
                   m,
                   e);
            None
        }
    }
}

fn try_format_status_msg(m: PrivMsg) -> Option<irc_msg> {
    match priv_msg(&m.chan, &m.msg) {
        Ok(m) => Some(m),
        Err(e) => {
            error!("failed to format a status message into a real privmsg {:?}, because {}",
                   m,
                   e);
            None
        }
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

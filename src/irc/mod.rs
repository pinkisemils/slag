use std::collections::{HashMap, HashSet};
use std::default::Default;
use std::error::Error;
use std::mem::discriminant;
use std::time::Duration;

use tokio_core::reactor;

use futures::{future, Future, Sink, Stream};
use futures::sync::mpsc;
use futures::sync::oneshot;

use message::{PrivMsg, SlackMsg};
use errors::{SlagErr, SlagErrKind};

use aatxe_irc;
use aatxe_irc::client::data::Config as AatxeConfig;
use aatxe_irc::client::{Client, IrcClient, PackedIrcClient};
use aatxe_irc::proto::command::Command as AatxeCmd;
use aatxe_irc::proto::message::Message as AatxeMsg;
use aatxe_irc::client::ext::ClientExt;
use tokio_timer::Timer;

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
    alt_nicks: Option<Vec<String>>,
    user: String,
    pass: Option<String>,
    use_ssl: Option<bool>,
    #[serde(skip)]
    pub channels: HashMap<String, String>,
}

#[derive(Debug)]
enum IrcOutMsg {
    FromSlack(SlackMsg),
    SenderShutdown,
}

#[derive(Debug)]
pub enum IrcFailure {
    Disconnect,
    Error(String),
    Connection(aatxe_irc::error::IrcError),
    BadConf(aatxe_irc::error::IrcError),
    CantIdentify(aatxe_irc::error::IrcError),
    Shutdown,
}

enum ConnResult {
    Recoverable(mpsc::Receiver<IrcOutMsg>, IrcFailure),
    Shutdown,
}

impl IrcCfg {
    fn conn_from_cfg(&self) -> AatxeConfig {
        AatxeConfig {
            nickname: Some(self.nick.clone()),
            alt_nicks: self.alt_nicks.clone(),
            server: Some(self.host.clone()),
            port: Some(self.port),
            channels: Some(self.channels.keys().cloned().collect()),
            use_ssl: Some(self.use_ssl.unwrap_or(true)),
            ping_time: Some(5),
            ping_timeout: Some(5),
            password: self.pass.clone(),
            ..Default::default()
        }
    }

    // TODO: consider futurizing this function
    fn run_once(
        &mut self,
        core: &mut reactor::Core,
        shutdown_chan: mpsc::Sender<IrcOutMsg>,
        in_stream: mpsc::Receiver<IrcOutMsg>,
        mut slack_chan: &mut mpsc::Sender<SlackMsg>,
    ) -> ConnResult {
        let cfg = self.conn_from_cfg();

        let future = match IrcClient::new_future(core.handle(), &cfg) {
            Ok(f) => f,
            Err(e) => return ConnResult::Recoverable(in_stream, IrcFailure::BadConf(e)),
        };

        // immediate connection errors (like no internet) will turn up here...
        let client = match core.run(future) {
            Ok(c) => c,
            Err(e) => return ConnResult::Recoverable(in_stream, IrcFailure::Connection(e)),
        };
        let PackedIrcClient(client, inner_fut) = client;
        let inner_fut = inner_fut.map_err(|e| {
            error!("error with connection: {:?}", e);
            ()
        });

        core.handle().spawn(inner_fut);

        if let Err(e) = client.identify() {
            return ConnResult::Recoverable(in_stream, IrcFailure::CantIdentify(e));
        }

        let sender = client.clone();
        let (sender_tx, sender_join) = oneshot::channel();
        let slack_sender = consume_sender(in_stream, sender)
            .then(|res| sender_tx.send(res))
            .map_err(|_| ());

        core.handle().spawn(slack_sender);

        let work = client
            .stream()
            // errors here mean a disconnection
            .map_err(|e| {
                error!("got IRC error {:?}", e);
                IrcFailure::Disconnect
            })
            .filter_map(handle_irc_msg)
            .for_each(|msg| match msg {
                Incoming::ForwardMsg(m) => {
                    try_send_to_slack(&mut slack_chan, m);
                    Ok(())
                }
                Incoming::Error(e) => Err(IrcFailure::Error(e)),
            })
            .then(|res: Result<(), IrcFailure>| {
                shutdown_chan.send(IrcOutMsg::SenderShutdown)
                    .then(|_| res)
            });

        info!("connected to IRC");
        let recv_err = core.run(work).err().unwrap_or(IrcFailure::Disconnect);
        let send_res = core.run(sender_join).unwrap();
        // If the sender errors, then slack has stopped sending messages.
        // This means that the gateway is shutting down.
        match send_res {
            Err(IrcFailure::Shutdown) => ConnResult::Shutdown,
            Err(_) => unreachable!(),
            Ok(slack_pipe) => ConnResult::Recoverable(slack_pipe, recv_err),
        }
        // unwrapping a future that is never cancelled
    }

    // TODO consider futurizing this function
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

        let mut err_state = ErrState::new();
        let timer = Timer::default();

        loop {
            match self.run_once(core, sink_in.clone(), sink_out, slack_chan) {
                ConnResult::Recoverable(slack_chan, err) => {
                    error!("got irc err- {:?}", err);
                    sink_out = slack_chan;
                    match err_state.handle_error(err) {
                        ErrResolution::Die(e) => {
                            return Err(SlagErrKind::IrcError(e).into());
                        }
                        ErrResolution::Backoff(0) => continue,
                        ErrResolution::Backoff(time) => {
                            let sleep = timer.sleep(Duration::from_secs(time));
                            core.run(sleep).expect("failed to sleep");
                            warn!("trying to reconnect to IRC");
                            continue;
                        }
                    }
                }
                ConnResult::Shutdown => {
                    return Ok(());
                }
            };
        }
    }
}

struct ErrState {
    err: Option<IrcFailure>,
    occurence: u32,
}

enum ErrResolution {
    Backoff(u64),
    Die(IrcFailure),
}

// Deals with backoff periods
impl ErrState {
    fn new() -> ErrState {
        ErrState {
            err: None,
            occurence: 0,
        }
    }

    // returns None if error is fatal
    // Returns Some(backoff) to backoff for a certain amount of time
    pub fn handle_error(&mut self, err: IrcFailure) -> ErrResolution {
        match &err {
            &IrcFailure::Disconnect => self.set_err(err),
            &IrcFailure::Connection(_) => self.set_err(err),
            &IrcFailure::BadConf(_) => ErrResolution::Die(err),
            &IrcFailure::Error(_) => ErrResolution::Die(err),
            &IrcFailure::CantIdentify(_) => ErrResolution::Die(err),
            &IrcFailure::Shutdown => ErrResolution::Die(err),
        }
    }

    fn set_err(&mut self, err: IrcFailure) -> ErrResolution {
        let errors_are_same = self.err
            .as_ref()
            .map(|prev_err| discriminant(&err) == discriminant(&prev_err))
            .unwrap_or(false);
        if !errors_are_same {
            self.occurence = 0;
            self.err = Some(err);
            ErrResolution::Backoff(0)
        } else {
            self.occurence += 1;
            ErrResolution::Backoff((2 as u64).pow(self.occurence - 1))
        }
    }
}

// A recursive future that will send messages from output_stream until it runs out or
// output_stream delivers an IrcOutMsg::SenderShutdown.
fn consume_sender(
    output_stream: mpsc::Receiver<IrcOutMsg>,
    sender: IrcClient,
) -> Box<Future<Item = mpsc::Receiver<IrcOutMsg>, Error = IrcFailure> + Send> {
    let next = output_stream.into_future();
    next.then(|res| {
        let (msg, stream) = res.unwrap();
        let msg = match msg {
            Some(m) => m,
            None => {
                // slack channel down
                return future::err(IrcFailure::Shutdown).boxed();
            }
        };
        let msg = match msg {
            IrcOutMsg::SenderShutdown => {
                return future::ok(stream).boxed();
            }
            IrcOutMsg::FromSlack(m) => m,
        };

        let msg = match handle_slack_msg(msg) {
            None => {
                return consume_sender(stream, sender);
            }
            Some(m) => m,
        };

        match sender.send(msg) {
            Ok(_) => consume_sender(stream, sender),
            Err(e) => {
                error!("encountered error sending to irc: {:?}", e);
                consume_sender(stream, sender)
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
            dropped_message, desc
        );
    }
}

#[derive(Debug)]
enum Incoming {
    ForwardMsg(SlackMsg),
    //Kick(String),
    Error(String),
}

fn handle_irc_msg(irc_msg: AatxeMsg) -> Option<Incoming> {
    let nick = irc_msg.source_nickname()?.to_string();
    let cmd = irc_msg.command;
    match cmd {
        AatxeCmd::PRIVMSG(target, msg) => Some(handle_privmsg(nick, target, msg)),
        //AatxeCmd::KICK(chans, users, comment) =>
        AatxeCmd::ERROR(err_message) => Some(Incoming::Error(err_message)),
        _ => None,
    }
}

fn handle_privmsg(nick: String, target: String, mut msg: String) -> Incoming {
    let action_preifx = "\x01ACTION";
    if msg.starts_with(action_preifx) {
        msg.splice(0..action_preifx.len() + 1, "");

        Incoming::ForwardMsg(SlackMsg::ActionMsg(PrivMsg {
            chan: target,
            msg: msg,
            nick: nick,
        }))
    } else {
        Incoming::ForwardMsg(SlackMsg::OutMsg(PrivMsg {
            chan: target,
            msg: msg,
            nick: nick,
        }))
    }
}

fn handle_slack_msg(slack_msg: SlackMsg) -> Option<AatxeCmd> {
    match slack_msg {
        SlackMsg::OutMsg(m) => try_format_out_msg(m),
        SlackMsg::ActionMsg(m) => try_format_action_msg(m),
        SlackMsg::StatusMsg(m) => try_format_status_msg(m),
    }
}

fn try_format_out_msg(m: PrivMsg) -> Option<AatxeCmd> {
    Some(AatxeCmd::PRIVMSG(
        m.chan,
        format!("[{}]: {}", m.nick, m.msg),
    ))
}

fn try_format_action_msg(m: PrivMsg) -> Option<AatxeCmd> {
    Some(AatxeCmd::PRIVMSG(m.chan, format!("[{}] {}", m.nick, m.msg)))
}

fn try_format_status_msg(m: PrivMsg) -> Option<AatxeCmd> {
    Some(AatxeCmd::PRIVMSG(m.chan, m.msg))
}

use irc::client::prelude::*;

use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::mpsc::channel;
use std::sync::mpsc::Receiver;
use std::marker::Sized;

use std::io::{Error, ErrorKind, Result};

use message::Msg;


// so this piece of code
// will initialize a 'listener' for Slack messages
//
// basically, a mio event loop per connection, with the listener distributing messages to the
// appropriate channel -
//  1. If an existing connection has a name already known, that connection is chosen
//  2. Otherwise, least used connection is used.
//
//  IrcServer structs must be initialized on startup. 
//  IrcServers can receive two enums

pub trait IrcHandler: Sized {
    fn current_nick(&self) -> &str;
    fn change_nick(&self, &str) -> Result<()>;
    fn send_msg(&self, &str, &str) -> Result<()>;
}
enum IrcMessage {
    Plain(String),
    NickChange(String)
}

struct IrcAgent {
    conn: IrcServer
}

impl IrcAgent {
    fn new(config:Config) -> Result<IrcAgent, > {
        Ok(IrcAgent {
            conn: try!(IrcServer::new("irc_config.json"))
        })
    }
}

impl IrcHandler for IrcAgent {
    fn current_nick(&self) -> &str{
        return self.conn.current_nickname();
    }
    fn change_nick(&self, new_nick: &str) -> Result<(), > {
        let current_nick = self.conn.current_nickname();
        self.conn.send_sanick(current_nick, new_nick)
    }
    fn send_msg(&self, msg:&str, chan:&str) -> Result<()>{
        self.conn.send_privmsg(msg, chan)
    }
}

//
//
//
//
//
pub struct IrcBroker<T> {
    clients: VecDeque<T>,
    //clients: VecDeque<&'a IrcHandler>,
    channel: Receiver<Msg>,
}

impl<T: IrcHandler> IrcBroker<T> {
    pub fn new(clients: VecDeque<T>, slack_chan: Receiver<Msg>) ->  IrcBroker<T> {
        IrcBroker{
            clients: clients,
            channel: slack_chan
        }
    }

    pub fn handle_msg(self) -> Result<()> {
        'main: loop {
            match try!(self.channel.recv()) {
                Msg::SlackMsg(nick, msg) => {
                    for client in self.clients {
                        if client.current_nick() == nick {
                            client.send_msg(msg, nick);
                            continue 'main;
                        }

                    }
                    
                }


            }
        }
    }
}


fn initialize_one () {
    let srv = IrcServer::new("irc_config.json").expect("Failed to initialize IRC connection");
    srv.identify().expect("Failed to identify");
    srv.send_privmsg("#zaebis", "zdarova");

}

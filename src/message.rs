use std::fmt;

#[derive(Debug)]
pub struct TransMsg {
    pub chan: String,
    pub text: String,
}

#[derive(Debug)]
pub enum Msg {
    IrcInMsg(PrivMsg),
    IrcOutMsg(PrivMsg),
    TranMsg(TransMsg),
    ResetIrc,
    Shutdown,
}

#[derive(Debug)]
pub enum SlackMsg {
    OutMsg(PrivMsg),
    Shutdown,
}

#[derive(Debug)]
pub struct PrivMsg {
    pub nick: String,
    pub chan: String,
    pub msg: String,
}

impl fmt::Display for PrivMsg {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "[{}] -> {}: {}", self.nick, self.chan, self.msg)
    }
}

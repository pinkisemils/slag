use std::fmt;

#[derive(Debug)]
pub enum SlackMsg {
    OutMsg(PrivMsg),
    StatusMsg(PrivMsg),
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

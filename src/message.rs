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

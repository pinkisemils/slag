#[derive(Debug)]
pub struct TransMsg {
    pub chan: String,
    pub text: String,
}

#[derive(Debug)]
pub enum Msg {
    TranMsg(TransMsg),
    ResetIrc,
    Shutdown,
}

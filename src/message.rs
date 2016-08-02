use std::collections::HashSet;
use std::sync::mpsc::channel;
use slack;

pub enum Msg {
    SlackMsg(String, String),
    Shutdown(String),
    SlackError(slack::Error)
}



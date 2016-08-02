use irc;
use slack_client;

#[derive(Deserialize,Serialize)]
pub struct Cfg {
    pub irc: irc::IrcCfg,
    pub slack: slack_client::SlackCfg,
}

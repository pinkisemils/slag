use irc;
use slack_client;

#[derive(Deserialize,Serialize)]
pub struct Cfg {
    #[serde(rename="irc")]
    pub irc_cfg: irc::IrcCfg,
    #[serde(rename="slack")]
    pub slack_cfg: slack_client::SlackCfg,
}

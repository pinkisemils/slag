use irc;
use slack_client;

use std::collections::HashMap;

#[derive(Deserialize,Serialize)]
pub struct Cfg {
    #[serde(rename="irc")]
    pub irc_cfg: irc::IrcCfg,
    #[serde(rename="slack")]
    pub slack_cfg: slack_client::SlackCfg,
    pub channels: HashMap<String, String>,
}

impl Cfg {
    pub fn get_cfg(self) -> (irc::IrcCfg, slack_client::SlackCfg) {
        let Cfg { mut irc_cfg, mut slack_cfg, channels } = self;
        let slack_chans = channels.iter()
            .map(|(irc, slack)| (slack.to_string(), irc.to_string()))
            .collect();
        slack_cfg.channels = slack_chans;
        irc_cfg.channels = channels;
        (irc_cfg, slack_cfg)
    }
}

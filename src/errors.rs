#[allow(unused_doc_comment)]

use slack;
use slack_hook;
use pircolate;
use config;
use tokio_irc_client;
use aatxe_irc;


error_chain!{
    types {
        SlagErr, SlagErrKind, SlagResult;
    }

    errors {
        SlackChanDown {
            description("slack channel depleted")
            display("slack channel depleted")
        }
        AatxeErr(e: aatxe_irc::error::IrcError) {
            description("underlying irc error: {:?}")
            display("underlying irc error: {:?}", e)
        }
    }

    links {
        Pircolate(pircolate::error::Error, pircolate::error::ErrorKind);
        TokioIrc(tokio_irc_client::error::Error, tokio_irc_client::error::ErrorKind);
        SlackHook(slack_hook::error::Error,slack_hook::error::ErrorKind);
    }

    foreign_links {
        Slack(slack::Error);
        Io(::std::io::Error);
        CfgError(config::ConfigError);
    }
}

#[allow(unused_doc_comment)]

use slack;
use slack_hook;
use config;
use irc;


error_chain!{
    types {
        SlagErr, SlagErrKind, SlagResult;
    }

    errors {
        IrcError(e: irc::IrcFailure) {
            description("irc connection failure")
            display("irc connection failed: {:?}", e)
        }
    }

    links {
        SlackHook(slack_hook::error::Error,slack_hook::error::ErrorKind);
    }

    foreign_links {
        Slack(slack::Error);
        Io(::std::io::Error);
        CfgError(config::ConfigError);
    }
}

use slack;
use slack_hook;
use pircolate;
use config;
use tokio_irc_client;

error_chain!{
    types {
        SlagErr, SlagErrKind, SlagResult;
    }

    errors {
        InvalidErr(t: String) {
            description("this is not expected")
            display("context: '{}'", t)
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

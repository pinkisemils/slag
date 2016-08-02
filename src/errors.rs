use slack;
use pircolate;
use config;
use tokio_irc_client;

error_chain!{
    types {
        SlagErr, SlagErrKind, SlagResult;
    }

    links {
        Pircolate(pircolate::error::Error, pircolate::error::ErrorKind);
        TokioIrc(tokio_irc_client::error::Error, tokio_irc_client::error::ErrorKind);
    }

    foreign_links {
        Slack(slack::Error);
        Io(::std::io::Error);
        CfgError(config::ConfigError);
    }
}

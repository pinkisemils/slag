# SLAGW
Slagw is a slack relay that connects it to a real chat network.

## Configuration
```yaml
irc:
    host: 127.0.0.1
    port: 6667
    nick: bot_mcbotface
    alt_nicks:
      - bot_mcbotface_
    user: bots
    pass: botpass
    use_ssl: true

slack:
    secret: $slack_token
    hook_url: integration hook for sending messages

# left side for IRC, right side for slack
channels:
    "#freenode": general
```
Optional options:
* `irc.use_ssl`, if omitted, defaults to `true`.
* `irc.pass`, if omitted, the client won't identify with services
* `irc.alt_nicks`, if omitted, the client will panic if the specified nick is
in use. Essentially, if there's a nick conflict, the library will panic.

## Unrelated dependencies
This application uses TLS. The TLS situation in Rust currently is a small
dumpster fire. Thus, make sure to have `pkg-config` in your path and a decent
`openssl` library installed.

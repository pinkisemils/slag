# SLAGW
Slagw is a slack relay that connects it to a real chat network.

## Configuration
```yaml
irc:
    host: 127.0.0.1
    port: 6667
    nick: bot_mcbotface
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

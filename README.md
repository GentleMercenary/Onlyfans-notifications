# OF-notifier
A tray application for Windows that gives you push notifications and instant downloads of new posts, messages and stories posted by models you subscribe to on Onlyfans.

## Setup
1. Clone this repository
2. [Install cargo/rustup](https://www.rust-lang.org/tools/install)
3. [Switch to the nightly toolchain](https://rust-lang.github.io/rustup/concepts/channels.html)
4. Fill out authentication header data in auth.json
5. run cargo install --release

The executable can be found in `target/release/`, but it expects auth.json to be in the same directory. You can move auth.json to this directory or create a shortcut to the executable that starts in the root directory of this repo.

Files are downloaded to data/{model_name}/{origin}/{content_type}/{filename} where <br>
origin = "Messages" | "Posts" | "Stories"<br>
content_type = "Audios" | "Images" | "Videos"<br>

This is the same format as the default for [this scraper](https://github.com/DIGITALCRIMINALs/OnlyFans), so you can symlink the `data` folder to wherever you store your scrapes or vice versa.

## Behaviour
When the connection gets interrupted (because of unstable network, wake up from sleep, ...) the application will stay running, but no notifications can be received until the user manually reconnects. To reconnect the websocket, click the tray icon. Once the connection is established, the icon will change, indicating that the connection was made succesfully.

| Connected | Disconnected |
|-----------|--------------|
|![Connected](res/icon.ico)|![Disconnected](res/icon2.ico)|

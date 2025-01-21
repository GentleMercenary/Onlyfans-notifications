# OF-notifier
A tray application for Windows that gives you push notifications and instant downloads of new posts, messages and stories posted by models you subscribe to on Onlyfans.

## Setup
1. Download the [latest release](https://github.com/GentleMercenary/Onlyfans-notifications/releases/latest)
2. Fill out authentication header data in auth.json
3. (Optional) Edit settings.json to your liking 
4. (Optional) provide CDM for downloading of drm-protected content
5. Run the executable

Files are downloaded to `data/{model_name}/{origin}/{content_type}/{filename}` where <br>
`origin = "Messages" | "Posts" | "Stories"`<br>
`content_type = "Audios" | "Images" | "Videos"`<br>

This is the same format as the default for [this scraper](https://github.com/DIGITALCRIMINALs/OnlyFans), so you can symlink the `data` folder to wherever you store your scrapes or vice versa.

## Settings
See [settings documentation](SETTINGS.md)
> [!CAUTION]
> The program will crash on startup with no logs if your settings are invalid. It is recommended you initially launch the program with the provided settings file, modify the settings as you like, and then use the icon context menu to reload the settings. In this case, if your settings are invalid the log will contain information on what exactly went wrong

## DRM
This program uses FFmpeg to decrypt and mux drm-protected files. If a system installation of FFmpeg is not found this program will download the latest version. <br>
This program will look for a CDM named "device.wvd" in the same path as the executable. If you have seperate client id and private key files you can use [this](https://emarsden.github.io/pssh-box-wasm/convert/) tool to convert them.

## Behaviour
When the connection gets interrupted (because of unstable network, wake up from sleep, ...) the application will stay running, but no notifications can be received until the user manually reconnects. To reconnect the websocket, click the tray icon. Once the connection is established, the icon will change, indicating that the connection was made succesfully.

| Connected | Disconnected |
|-----------|--------------|
|![Connected](icons/icon.ico)|![Disconnected](icons/icon2.ico)|

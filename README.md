# OF-notifier
A tray application for Windows that gives you push notifications and instant downloads of new posts, messages and stories posted by models you subscribe to on Onlyfans.

## Setup
1. Download the [latest release](https://github.com/GentleMercenary/Onlyfans-notifications/releases/latest)
2. Fill out authentication header data in auth.json
3. (Optional) Edit settings.json to your liking 
4. Run the executable

Files are downloaded to `data/{model_name}/{origin}/{content_type}/{filename}` where <br>
`origin = "Messages" | "Posts" | "Stories"`<br>
`content_type = "Audios" | "Images" | "Videos"`<br>

This is the same format as the default for [this scraper](https://github.com/DIGITALCRIMINALs/OnlyFans), so you can symlink the `data` folder to wherever you store your scrapes or vice versa.

## Settings
You can choose whether or not you receive notifications, download data or like new content in settings.json. Setting any to true or false enables/disables this functionality. Aside from this you can list the usernames for whom you wish these features to be active.
For more granular control, you can specify for which types of content to enable that feature. <br>
Allowed content types are `posts, messages, stories, notifications, streams`. Any missing fields will default to being `false`.

#### Example
If you wanted to receive notifications for your entire following list, only download the media for certain models, and like every post on your feed alongside all stories for a specific user, your settings.json would look like this:
```
{
    "notify": true,
    "download": ["username1", "username2"],
    "like": {
        "posts": true,
        "stories": ["username3"]
    }
}
```
Note that a model's username and name can be different. The username can most easily be found in the profile url: `onlyfans.com/{username}`

## Behaviour
When the connection gets interrupted (because of unstable network, wake up from sleep, ...) the application will stay running, but no notifications can be received until the user manually reconnects. To reconnect the websocket, click the tray icon. Once the connection is established, the icon will change, indicating that the connection was made succesfully.

| Connected | Disconnected |
|-----------|--------------|
|![Connected](icons/icon.ico)|![Disconnected](icons/icon2.ico)|

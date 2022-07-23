#![allow(dead_code)]

use super::client;

use std::{io::Cursor, fs::File, collections::HashMap, time, path::Path};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, offset::Utc};
use winrt_notification::{Toast, IconCrop};

#[derive(Serialize)]
pub struct ConnectMessage<'a> {
    pub act: &'static str,
    pub token: &'a str
}

#[derive(Serialize)]
pub struct GetOnlinesMessage {
    pub act: &'static str,
    pub ids: &'static [&'static str]
}

#[derive(Deserialize)]
pub struct ErrorMessage { 
    error: i32,
}

#[derive(Deserialize)]
pub struct ConnectedMessage<'a> { 
    connected: bool,
    v: &'a str
}

#[derive(Deserialize)]
pub struct PostPublishedMessage<'a> { 
    id: &'a str,
    show_posts_in_feed: bool,
    user_id: &'a str
}

#[derive(Deserialize, Clone)]
pub struct User {
    pub avatar: String,
    pub id: i32,
    pub name: String,
    pub username: String,

    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage<'a>{
    pub text: &'a str,
    pub from_user: User,

    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryMessage {
    pub id: i32,
    pub user_id: i32,

    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaggedMessageType<'a> {
    #[serde(borrow)]
    PostPublished(PostPublishedMessage<'a>),
    #[serde(borrow)]
    Api2ChatMessage(ChatMessage<'a>),
    Stories(Vec<StoryMessage>)
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum MessageType<'a> {
    #[serde(borrow)]
    Tagged(TaggedMessageType<'a>),
    #[serde(borrow)]
    Connected(ConnectedMessage<'a>),
    Error(ErrorMessage),
}

impl<'a> MessageType<'a> {
    pub async fn handle_message(&self) -> () {

        let log = |s: &str| {
            let sys_time: DateTime<Utc> = time::SystemTime::now().into();
            println!("[{}] {}", sys_time.format("%d/%m/%Y %T"), s);
        };

        let handle_non_content = |s: &str| {
            log(s);
            Toast::new(Toast::POWERSHELL_APP_ID)
            .title("OF Notifier")
            .text1(&s)
            .show().unwrap();
        };

        let handle_content = |user: User, content_type: String, content_body: Option<String>| async move {
            let avatar_path = format!("avatars/{}", user.id);
            let mut avatar_file = File::create(&avatar_path).unwrap();
            fetch_avatar(&user.avatar, &mut avatar_file).await;

            let s = format!("New {} from {}", content_type, user.name);

            let mut toast = Toast::new(Toast::POWERSHELL_APP_ID)
            .title(&s)
            .icon(&Path::new(env!("CARGO_MANIFEST_DIR")).join(avatar_path), IconCrop::Circular, "avatar");

            if let Some(body) = content_body {
                toast = toast.text1(&body);
            }
            
            toast.show().unwrap();
        };

        match self {
            Self::Connected(_) => { 
                handle_non_content("Connection established");
            },
            Self::Error(msg) =>  {
                handle_non_content(&format!("Error: {}", &msg.error));
            },
            Self::Tagged(TaggedMessageType::PostPublished(msg)) => {
                let user = get_user(&msg.user_id).await;
                handle_content(user, "post".to_owned(), None).await;

            },
            Self::Tagged(TaggedMessageType::Api2ChatMessage(msg)) => {
                handle_content(msg.from_user.to_owned(), "message".to_owned(), Some(msg.text.to_owned())).await;
            },
            Self::Tagged(TaggedMessageType::Stories(msg)) => {
                let user = get_user(&msg[0].user_id.to_string()).await;
                handle_content(user, "story".to_owned(), None).await;
            }
        };
    }
}

async fn get_user(user_id: &str) -> User {
    let response = client::get_json(&["https://onlyfans.com/api2/v2/users/list?x[]=", user_id].concat()).await.unwrap();
    let response_json: serde_json::Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
    serde_json::from_value(response_json[user_id].clone()).unwrap()
}

async fn fetch_avatar(url: &str, file: &mut File) {
    let image_response = reqwest::get(url).await.unwrap();
    std::io::copy(&mut Cursor::new(image_response.bytes().await.unwrap()), file).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{Rng, distributions::{Alphanumeric, DistString}};

    #[test]
    fn connected_message() {
        let connected = rand::thread_rng().gen_bool(0.5);
        let v = Alphanumeric.sample_string(&mut rand::thread_rng(), 16);

        let incoming = format!("{{\"connected\": {}, \"v\": \"{}\"}}",
            connected, v);

        match serde_json::from_str::<MessageType>(&incoming).unwrap() {
            MessageType::Connected(msg) => {
                assert_eq!(connected, msg.connected);
                assert_eq!(v, msg.v);
            }
            _ => panic!("Did not parse to correct type")
        }
    }

    #[test]
    fn post_published_message() {
        let id = rand::thread_rng()
            .gen_range(99999..9999999)
            .to_string();

        let show_posts = rand::thread_rng().gen_bool(0.5);
        let user_id = rand::thread_rng()
            .gen_range(99999..9999999)
            .to_string();

        let incoming = format!("{{ \"post_published\": {{ \"id\": \"{}\", \"show_posts_in_feed\": {}, \"user_id\": \"{}\" }}}}",
            id, show_posts, user_id);

        match serde_json::from_str::<MessageType>(&incoming).unwrap() {
            MessageType::Tagged(TaggedMessageType::PostPublished(msg)) => {
                assert_eq!(id, msg.id);
                assert_eq!(show_posts, msg.show_posts_in_feed);
                assert_eq!(user_id, msg.user_id);
            }
            _ => panic!("Did not parse to correct type")
        }
    }

    #[test]
    fn chat_message() {
        let text = Alphanumeric.sample_string(&mut rand::thread_rng(), 16);
        let avatar = Alphanumeric.sample_string(&mut rand::thread_rng(), 16);
        let id = rand::thread_rng().gen_range(9999..999999);
        let name = Alphanumeric.sample_string(&mut rand::thread_rng(), 16);
        let username = Alphanumeric.sample_string(&mut rand::thread_rng(), 16);

        let incoming = format!("{{ \"api2_chat_message\": {{ \"text\": \"{}\", \"fromUser\": {{ \"avatar\": \"{}\", \"id\": {}, \"name\": \"{}\", \"username\": \"{}\" }} }} }}",
            text, avatar, id, name, username);

        match serde_json::from_str::<MessageType>(&incoming).unwrap() {
            MessageType::Tagged(TaggedMessageType::Api2ChatMessage(msg)) => {
                assert_eq!(text, msg.text);
                assert_eq!(avatar, msg.from_user.avatar);
                assert_eq!(id, msg.from_user.id);
                assert_eq!(name, msg.from_user.name);
                assert_eq!(username, msg.from_user.username);
            }
            _ => panic!("Did not parse to correct type")
        }
    }

}
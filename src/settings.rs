use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct Settings {
	pub notify: Whitelist,
	pub download: Whitelist,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum Whitelist {
	Select(Vec<String>),
	Full(bool),
}

impl Settings {
	pub fn should_notify(&self, username: &str) -> bool {
		match &self.notify {
			Whitelist::Select(whitelist) => whitelist.iter().find(|&s| s == username).is_some(),
			Whitelist::Full(b) => *b,
		}
	}

	pub fn should_download(&self, username: &str) -> bool {
		match &self.download {
			Whitelist::Select(whitelist) => whitelist.iter().find(|&s| s == username).is_some(),
			Whitelist::Full(b) => *b,
		}
	}
}

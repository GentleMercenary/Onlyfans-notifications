use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct Settings {
	pub notify: Whitelist,
	pub download: Whitelist,
	pub like: Whitelist
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
			Whitelist::Select(whitelist) => whitelist.iter().any(|s| s == username),
			Whitelist::Full(b) => *b,
		}
	}

	pub fn should_download(&self, username: &str) -> bool {
		match &self.download {
			Whitelist::Select(whitelist) => whitelist.iter().any(|s| s == username),
			Whitelist::Full(b) => *b,
		}
	}

	pub fn should_like(&self, username: &str) -> bool {
		match &self.like {
			Whitelist::Select(whitelist) => whitelist.iter().any(|s| s == username),
			Whitelist::Full(b) => *b,
		}
	}
}

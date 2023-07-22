use chrono::Utc;
use rand_distr::{Distribution, Standard};
use serde::Serialize;

pub mod user;
pub mod media;
pub mod content;
pub mod socket;

#[derive(Debug, Serialize)]
enum Pages {
	Collections,
	Subscribes,
	Profile,
	Chats,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClickStats {
	page: Pages,
	block: &'static str,
	event_time: String
}

impl Distribution<ClickStats> for Standard {
	fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> ClickStats {
		ClickStats {
			page: match rng.gen_range(0..=3) {
				0 => Pages::Collections,
				1 => Pages::Subscribes,
				2 => Pages::Profile,
				_ => Pages::Chats
			},
			block: "Menu",
			event_time: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
		}
	}
}
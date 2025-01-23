pub mod selection;
pub mod actions;

use log::LevelFilter;
use serde::Deserialize;
use actions::Actions;

const fn default_log_level() -> LevelFilter {
	LevelFilter::Info
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Settings {
	pub actions: Actions,
	pub reconnect: bool,
	#[serde(default = "default_log_level")]
	pub log_level: LevelFilter
}

impl Default for Settings {
	fn default() -> Self {
		Self {
			actions: Actions::default(),
			reconnect: true,
			log_level: default_log_level()
		}
	}
}
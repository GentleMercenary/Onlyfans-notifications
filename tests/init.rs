use log::LevelFilter;
use simplelog::{TermLogger, Config, TerminalMode, ColorChoice};

pub fn init_log() {
	TermLogger::init(
		LevelFilter::Debug,
		Config::default(),
		TerminalMode::Mixed,
		ColorChoice::Auto
	).unwrap();
}
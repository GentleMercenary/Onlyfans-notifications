use log::LevelFilter;
use simplelog::{TermLogger, Config, TerminalMode, ColorChoice};

pub fn init_log() {
	let _ = TermLogger::init(
		LevelFilter::Debug,
		Config::default(),
		TerminalMode::Mixed,
		ColorChoice::Auto
	);
}
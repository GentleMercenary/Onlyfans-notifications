use std::{fs::{File, OpenOptions}, io::Write};

use http::header;
use httpdate::{fmt_http_date, parse_http_date};
use serde::Deserialize;
use thiserror::Error;


#[derive(Deserialize, Debug, Clone)]
pub struct DynamicRules {
	#[serde(rename = "app-token")]
	pub app_token: String,
	pub static_param: String,
	pub prefix: String,
	pub suffix: String,
	pub checksum_constant: i32,
	pub checksum_indexes: Vec<usize>,
}

#[derive(Debug, Error)]
pub enum RulesError {
	#[error("Remote rules were not modified")]
	NotModified,
	#[error("{0}")]
	Request(#[from] reqwest::Error),
	#[error("{0}")]
	Parse(#[from] serde_json::Error),
	#[error("{0}")]
	IO(#[from] std::io::Error)
}

pub struct DynamicRulesProvider {
	client: reqwest::Client
}

impl DynamicRulesProvider {
	pub fn new() -> Result<Self, reqwest::Error> {
		Ok(Self { 
			client: reqwest::Client::builder().build()?
		})
	}

	pub async fn read(&self) -> Result<DynamicRules, RulesError> {
		let local = match File::open("rules.json") {
			Ok(file) => {
				let modified = file.metadata().and_then(|m| m.modified()).ok();
				let rules = serde_json::from_reader::<&File, DynamicRules>(&file)
					.inspect_err(|e| warn!("Local rules.json could not be parsed: {e}"))
					.ok();
				Option::zip(rules, modified)
			}
			Err(err) => {
				warn!("Could not open local rules.json file: {err}");
				None
			}
		};

		let remote = async {
			let mut req = self.client.get("https://git.ofdl.tools/sim0n00ps/dynamic-rules/raw/branch/main/rules.json");
	
			if let Some((_, modified)) = local {
				req = req.header(header::IF_MODIFIED_SINCE, fmt_http_date(modified));
			}
	
			let response = req.send().await?;

			if response.status() == reqwest::StatusCode::NOT_MODIFIED {
				return Err(RulesError::NotModified);
			}

			let modified = response
				.headers()
				.get(header::LAST_MODIFIED)
				.and_then(|h| h.to_str().ok())
				.and_then(|s| parse_http_date(s).ok());
		
			let body = response.text().await?;
			let rules = serde_json::from_str::<DynamicRules>(&body)?;

			if let Ok(mut file) = OpenOptions::new()
				.create(true)
				.write(true)
				.truncate(true)
				.open("rules.json")
			{
				file.write_all(body.as_bytes())?;
				if let Some(modified) = modified { file.set_modified(modified)? }
			}

			Ok(rules)
		}.await;

		remote.or_else(|err| local.map(|(rules, _)| rules).ok_or(err))
	}
}
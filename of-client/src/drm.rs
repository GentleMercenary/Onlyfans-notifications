#![allow(async_fn_in_trait)]

use std::time::SystemTime;
use futures::TryFutureExt;
use http::header::LAST_MODIFIED;
use httpdate::parse_http_date;
use log::*;
use minidom::Element;
use reqwest::Url;
use reqwest_cookie_store::RawCookie;
use widevine::{Cdm, Key, KeyType, LicenseType, Pssh};
use thiserror::Error;

use crate::{media::DRM, OFClient};

const NS: &str = "urn:mpeg:dash:schema:mpd:2011";
const CENC: &str = "urn:mpeg:cenc:2013";

#[derive(Error, Debug)]
pub enum MPDFetchError {
	#[error("{0}")]
	Reqwest(#[from] reqwest_middleware::Error),
	#[error("{0}")]
	Parse(#[from] minidom::Error),
	#[error("Value {0} not found in document")]
	ValueNotFound(String)
}

#[derive(Error, Debug)]
pub enum KeyFetchError {
	#[error("{0}")]
	Reqwest(#[from] reqwest_middleware::Error),
	#[error("{0}")]
	Widevine(#[from] widevine::Error)
}

pub struct MPDData {
	pub base_url: String,
	pub pssh: Pssh,
	pub last_modified: Option<SystemTime>
}

impl OFClient {
	pub async fn get_mpd_data(&self, drm: &DRM) -> Result<MPDData, MPDFetchError> {
		let mpd = Url::parse(&drm.manifest.dash).unwrap();

		{
			let signature = &drm.signature.dash;
			let headers = self.headers.read().unwrap();
			let mut write_lock = headers.cookie.write().unwrap();
			write_lock.insert_raw(&RawCookie::new("CloudFront-Policy", &signature.policy), &mpd).unwrap();
			write_lock.insert_raw(&RawCookie::new("CloudFront-Signature", &signature.signature), &mpd).unwrap();
			write_lock.insert_raw(&RawCookie::new("CloudFront-Key-Pair-Id", &signature.key_pair), &mpd).unwrap();
		}
		
		let response = self.get(mpd)
			.send()
			.await?;

		let last_modified = response.headers().get(LAST_MODIFIED)
			.and_then(|header| header.to_str().ok())
			.and_then(|v| parse_http_date(v).ok());

		let xml = response.text().await.map_err(Into::<reqwest_middleware::Error>::into)?;
		let root = xml.parse::<Element>()?;
		let adaptation_set = root
		.get_child("Period", NS)
		.ok_or_else(|| MPDFetchError::ValueNotFound("Period".to_string()))?
		.children()
		.find(|e| e.name() == "AdaptationSet"
			&& e.attrs()
				.any(|(name, value)| name == "mimeType" && value == "video/mp4")
		)
		.ok_or_else(|| MPDFetchError::ValueNotFound("AdaptationSet".to_string()))?;

		let pssh = Pssh::from_b64(
			&adaptation_set
			.children()
			.find(|e| e.name() == "ContentProtection"
				&& e.attrs()
					.any(|(name, value)| name == "schemeIdUri" && value == "urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed")
			)
			.ok_or_else(|| MPDFetchError::ValueNotFound("ContentProtection".to_string()))?
			.get_child("pssh", CENC)
			.ok_or_else(|| MPDFetchError::ValueNotFound("pssh".to_string()))?
			.text()
		).unwrap();
		
		let base_url = adaptation_set
			.children()
			.filter(|e| e.name() == "Representation")
			.max_by_key(|e| e.attr("bandwidth").and_then(|v| v.parse::<u64>().ok()))
			.ok_or_else(|| MPDFetchError::ValueNotFound("Representation".to_string()))?
			.get_child("BaseURL", NS)
			.ok_or_else(|| MPDFetchError::ValueNotFound("BaseURL".to_string()))?
			.text();

		Ok(MPDData { base_url, pssh, last_modified })
	}

	pub async fn get_decryption_key(&self, cdm: &Cdm, license_url: &str, pssh: Pssh) -> Result<Key, KeyFetchError> {
		let request = cdm
			.open()
			.get_license_request(pssh, LicenseType::STREAMING)?;
		
		let challenge = request.challenge()?;

		let license = self.post(license_url)
			.body(challenge)
			.send()
			.and_then(|response| response.bytes().map_err(Into::into))
			.await?;

		let keys = request.get_keys(&license)?;
		let key = keys.first_of_type(KeyType::CONTENT)?;
		Ok(key.clone())
	}

	pub fn mpd_header(&self, manifest_url: &str) -> String {
		let url = Url::parse(manifest_url).unwrap();

		let mut header_str = String::new();
		header_str.push_str("Cookie: ");
		
		let headers = self.headers.read().unwrap();

		for (name, val) in headers.cookie.read().unwrap().get_request_values(&url) {
			header_str.push_str(name);
			header_str.push('=');
			header_str.push_str(val);
			header_str.push(';');
		}

		header_str.push_str("User-Agent: ");
		header_str.push_str(&headers.user_agent);
		header_str	
	}
}
use chrono::Utc;
use reqwest::Response;
use serde::Serialize;

use crate::client::{OFClient, Authorized};

pub mod user;
pub mod media;
pub mod content;
pub mod socket;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClickStats {
    page: &'static str,
    block: &'static str,
    event_time: String
}

impl Default for ClickStats {
    fn default() -> Self {
        ClickStats {
            page: "Profile",
            block: "Menu",
            event_time: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        }
    }
}

impl OFClient<Authorized> {
    pub async fn click_stats(&self) -> anyhow::Result<Response> {
        self.post("https://onlyfans.com/api2/v2/users/clicks-stats", Some(&ClickStats::default())).await
    }
}
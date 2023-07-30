#![allow(dead_code)]

pub mod socket;

use of_client::content;
use winrt_toast::{Toast, content::text::TextPlacement, Text};

pub trait ToToast {
    fn to_toast(&self) -> Toast;
}

impl ToToast for content::Post {
    fn to_toast(&self) -> Toast {
        let mut toast = Toast::new();
		toast.text2(&self.raw_text);

		if let Some(price) = self.price && price > 0f32 {
			toast
			.text3(Text::new(format!("${price:.2}"))
			.with_placement(TextPlacement::Attribution));
		}

		toast
    }
}

impl ToToast for content::Chat {
    fn to_toast(&self) -> Toast {
        let mut toast = Toast::new();
		toast.text2(&self.text);

		if let Some(price) = self.price && price > 0f32 {
			toast
			.text3(Text::new(format!("${price:.2}"))
			.with_placement(TextPlacement::Attribution));
		}

		toast
    }
}

impl ToToast for content::Story {
    fn to_toast(&self) -> Toast {
        Toast::new()
    }
}

impl ToToast for content::Notification {
    fn to_toast(&self) -> Toast {
        let mut toast = Toast::new();
		toast.text2(&self.text);
		
		toast
    }
}

impl ToToast for content::Stream {
    fn to_toast(&self) -> Toast {
        let mut toast = Toast::new();

		toast
		.text2(&self.title)
		.text3(&self.description);

		toast
    }
}
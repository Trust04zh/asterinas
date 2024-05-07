// SPDX-License-Identifier: MPL-2.0

use core::sync::atomic::{AtomicU8, Ordering::Relaxed};

pub(crate) static CENTURY_REGISTER: AtomicU8 = AtomicU8::new(0);

pub fn init() {
    log::warn!("TODO: rtc.rs:init");
}

pub fn get_cmos(reg: u8) -> u8 {
    log::warn!("TODO: rtc.rs:get_cmos");
    1
}

pub fn is_updating() -> bool {
    log::warn!("TODO: rtc.rs:is_updating");
    false
}

// SPDX-License-Identifier: MPL-2.0

use alloc::{boxed::Box, collections::BinaryHeap, sync::Arc, vec::Vec};
use core::{
    any::Any,
    sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
};

use spin::Once;

use crate::{arch::boot::DEVICE_TREE, sync::SpinLock, trap::IrqLine};

/// The timer frequency (Hz). Here we choose 1000Hz since 1000Hz is easier for unit conversion and
/// convenient for timer. What's more, the frequency cannot be set too high or too low, 1000Hz is
/// a modest choice.
///
/// For system performance reasons, this rate cannot be set too high, otherwise most of the time
/// is spent executing timer code.
///
/// Due to hardware limitations, this value cannot be set too low; for example, PIT cannot accept
/// frequencies lower than 19Hz = 1193182 / 65536 (Timer rate / Divider)
pub const TIMER_FREQ: u64 = 1000;

pub static TIMER_IRQ_NUM: AtomicU8 = AtomicU8::new(32);
pub static TIMEBASE_FREQ: AtomicU64 = AtomicU64::new(1);
pub static TIMER_STEP: AtomicU64 = AtomicU64::new(1);
pub static TICK: AtomicU64 = AtomicU64::new(0);

static TIMER_IRQ: Once<IrqLine> = Once::new();

pub fn init() {
    let timer_freq = DEVICE_TREE.get().unwrap().cpus().next().unwrap().timebase_frequency() as u64;
    TIMEBASE_FREQ.store(timer_freq, Ordering::Relaxed);
    TIMER_STEP.store(timer_freq / TIMER_FREQ, Ordering::Relaxed);
    log::debug!("Timer initialized with frequency: {} Hz, timer step: {} Hz", timer_freq, TIMER_STEP.load(Ordering::Relaxed));
    TIMEOUT_LIST.call_once(|| SpinLock::new(BinaryHeap::new()));
    let _ = TIMEOUT_LIST.get().unwrap();
    set_next_timer();
}

fn set_next_timer() {
    sbi_rt::set_timer(TIMER_STEP.load(Ordering::Relaxed));
}

pub fn timer_callback() {
    let current_ticks = TICK.fetch_add(1, Ordering::SeqCst);

    let callbacks = {
        let mut callbacks = Vec::new();
        let mut timeout_list = TIMEOUT_LIST.get().unwrap().lock_irq_disabled();

        while let Some(t) = timeout_list.peek() {
            if t.is_cancelled() {
                // Just ignore the cancelled callback
                timeout_list.pop();
            } else if t.expire_ticks <= current_ticks {
                callbacks.push(timeout_list.pop().unwrap());
            } else {
                break;
            }
        }
        callbacks
    };

    for callback in callbacks {
        (callback.callback)(&callback);
    }

    set_next_timer();
}

static TIMEOUT_LIST: Once<SpinLock<BinaryHeap<Arc<TimerCallback>>>> = Once::new();

pub struct TimerCallback {
    expire_ticks: u64,
    data: Arc<dyn Any + Send + Sync>,
    callback: Box<dyn Fn(&TimerCallback) + Send + Sync>,
    is_cancelled: AtomicBool,
}

impl TimerCallback {
    fn new(
        timeout_ticks: u64,
        data: Arc<dyn Any + Send + Sync>,
        callback: Box<dyn Fn(&TimerCallback) + Send + Sync>,
    ) -> Self {
        Self {
            expire_ticks: timeout_ticks,
            data,
            callback,
            is_cancelled: AtomicBool::new(false),
        }
    }

    pub fn data(&self) -> &Arc<dyn Any + Send + Sync> {
        &self.data
    }

    /// Whether the set timeout is reached
    pub fn is_expired(&self) -> bool {
        let current_tick = TICK.load(Ordering::Acquire);
        self.expire_ticks <= current_tick
    }

    /// Cancel a timer callback. If the callback function has not been called,
    /// it will never be called again.
    pub fn cancel(&self) {
        self.is_cancelled.store(true, Ordering::Release);
    }

    // Whether the timer callback is cancelled.
    fn is_cancelled(&self) -> bool {
        self.is_cancelled.load(Ordering::Acquire)
    }
}

impl PartialEq for TimerCallback {
    fn eq(&self, other: &Self) -> bool {
        self.expire_ticks == other.expire_ticks
    }
}

impl Eq for TimerCallback {}

impl PartialOrd for TimerCallback {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TimerCallback {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.expire_ticks.cmp(&other.expire_ticks).reverse()
    }
}

/// add timeout task into timeout list, the frequency see const TIMER_FREQ
///
/// user should ensure that the callback function cannot take too much time
///
pub fn add_timeout_list<F, T>(timeout: u64, data: T, callback: F) -> Arc<TimerCallback>
where
    F: Fn(&TimerCallback) + Send + Sync + 'static,
    T: Any + Send + Sync,
{
    let timer_callback = TimerCallback::new(
        TICK.load(Ordering::Acquire) + timeout,
        Arc::new(data),
        Box::new(callback),
    );
    let arc = Arc::new(timer_callback);
    TIMEOUT_LIST
        .get()
        .unwrap()
        .lock_irq_disabled()
        .push(arc.clone());
    arc
}

/// The time since the system boots up.
/// The currently returned results are in milliseconds.
pub fn read_monotonic_milli_seconds() -> u64 {
    todo!()
}

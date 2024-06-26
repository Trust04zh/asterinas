// SPDX-License-Identifier: MPL-2.0

pub mod mmio;
pub mod pci;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BusProbeError {
    DeviceNotMatch,
    ConfigurationSpaceError,
}

pub fn init() {
    // pci::init();
    mmio::init();
}

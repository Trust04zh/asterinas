// SPDX-License-Identifier: MPL-2.0

use clap::ValueEnum;

use std::path::PathBuf;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrubScheme {
    /// The path of `grub_mkrecue`. Only needed if `boot.method` is `grub`
    pub grub_mkrescue: Option<PathBuf>,
    /// The boot protocol specified in the GRUB configuration
    pub boot_protocol: Option<BootProtocol>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum BootProtocol {
    Linux,
    Multiboot,
    Multiboot2,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grub {
    pub grub_mkrescue: PathBuf,
    pub boot_protocol: BootProtocol,
}

impl Default for Grub {
    fn default() -> Self {
        Grub {
            grub_mkrescue: PathBuf::from("grub-mkrescue"),
            boot_protocol: BootProtocol::Multiboot2,
        }
    }
}

impl GrubScheme {
    pub fn inherit(&mut self, from: &Self) {
        if self.grub_mkrescue.is_none() {
            self.grub_mkrescue = from.grub_mkrescue.clone();
        }
        if self.boot_protocol.is_none() {
            self.boot_protocol = from.boot_protocol;
        }
    }

    pub fn finalize(self) -> Grub {
        Grub {
            grub_mkrescue: self.grub_mkrescue.unwrap_or(PathBuf::from("grub-mkrescue")),
            boot_protocol: self.boot_protocol.unwrap_or(BootProtocol::Multiboot2),
        }
    }
}

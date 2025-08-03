#![no_std]
#![cfg_attr(target_arch = "aarch64", feature(stdarch_arm_hints))]
mod cmd;
mod dma;
mod error;
#[cfg(feature = "std")]
mod huge_tables;
mod nvme;
#[cfg(feature = "std")]
mod pci;
mod prp;
mod queue_pairs;
mod queues;

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub use dma::Allocator;
pub use error::Error;
#[cfg(feature = "std")]
pub use huge_tables::{HugePageAllocator, HUGE_PAGE_SIZE};
pub use nvme::{ControllerInformation, Namespace, NamespaceId, NvmeDevice};
pub use queue_pairs::{IoQueuePair, IoQueuePairId};

#[cfg(feature = "std")]
pub fn new_pci_and_huge(pci_address: &str) -> Result<NvmeDevice<HugePageAllocator>, Error> {
    let allocator = HugePageAllocator {};
    let nvme = NvmeDevice::from_pci_address(pci_address, HUGE_PAGE_SIZE, allocator)?;
    Ok(nvme)
}

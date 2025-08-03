use crate::nvme::NamespaceId;
use crate::queue_pairs::IoQueuePairId;
use alloc::boxed::Box;
use alloc::string::String;
use core::fmt;

#[derive(Debug)]
pub enum Error {
    Allocate(Box<dyn core::error::Error>),
    Deallocate(Box<dyn core::error::Error>),
    TranslateVirtualToPhysical(Box<dyn core::error::Error>),
    Layout(core::alloc::LayoutError),
    NotABlockDevice(String),
    MaximumQueueEntriesSupportedInvalidlyZero,
    NvmCommandSetNotSupported,
    MemoryPageSizeMinimumBiggerThanMaximum(u64, u64),
    PageSizeLessThanNvmeMinimum(usize),
    PageSizeMoreThanNvmeMaximum(usize),
    PageSizeLessThanControllerMinimum(usize, u64),
    PageSizeMoreThanControllerMaximum(usize, u64),
    PageSizeNotAPowerOfTwo(usize),
    ControllerTypeInvalid(String),
    NamespaceDoesNotExist(NamespaceId),
    NumberOfQueueEntriesLessThanTwo(u32),
    NumberOfQueueEntriesMoreThanMaximum(u32, u32),
    MaximumNumberOfQueuesReached,
    IoQueuePairDoesNotExist(IoQueuePairId),
    MemoryAccessOutOfBounds,
    UnixPciError(Box<dyn core::error::Error>),
    VirtualAddressIsNotDwordAligned(usize),
    VirtualAddressIsNotPageAligned(usize),
    BufferLengthBiggerThanMaximumTransferSize(usize, usize),
    BufferLengthNotAMultipleOfNamespaceBlockSize(usize, u64),
    IoCompletionQueueFailure(u16),
    SubmissionQueueFull,
    CompletionQueueCompletionFailure,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Allocate(error) => write!(f, "Allocation error: {error}."),
            Error::Deallocate(error) => write!(f, "Deallocation error: {error}."),
            Error::TranslateVirtualToPhysical(error) => write!(f, "Translation error: {error}."),
            Error::Layout(error) => write!(f, "{error}"),
            Error::NotABlockDevice(pci_address) => write!(
                f,
                "The device at PCI address {pci_address} is not a block device."
            ),
            Error::MaximumQueueEntriesSupportedInvalidlyZero => write!(
                f,
                "The value of \"Maximum Queue Entries Supported (MQES)\" in the
                capabilities register (CAP) is invalidly set to 0."
            ),
            Error::NvmCommandSetNotSupported => write!(f, "The device does not support the NVM command set."),
            Error::MemoryPageSizeMinimumBiggerThanMaximum(minimum, maximum) => write!(f,
                "The value of \"Memory Page Size Minimum (MPSMIN)\" ({minimum}) is bigger than \
                 the value of \"Memory Page Size Maximum (MPSMAX)\" ({maximum}) in the capabilities register (CAP)."
            ),
            Error::PageSizeLessThanNvmeMinimum(page_size) => write!(f,
                "The page size used ({page_size:X}) is less than \
                the lowest minimum page size of 4 KiB (2^12 B)."
            ),
            Error::PageSizeMoreThanNvmeMaximum(page_size) => write!(f,
                "The page size used ({page_size:X}) is more than \
                the highest maximum page size of 128 MiB (2^28 B)."
            ),
            Error::PageSizeLessThanControllerMinimum(page_size, minimum) => write!(f,
                "The page size used ({page_size:X}) is less than \
                the minimum memory page size of the controller ({minimum:X})."
            ),
            Error::PageSizeMoreThanControllerMaximum(page_size, maximum) => write!(f,
                "The page size used ({page_size:X}) is more than \
                the maximum memory page size of the controller ({maximum:X})."
            ),
            Error::PageSizeNotAPowerOfTwo(page_size) => write!(f,
                "The page size used ({page_size:X}) is not a power of two."
            ),
            Error::ControllerTypeInvalid(type_name) => write!(f,
                "The controller type is not \"I/O controller\" but instead \"{type_name}\"."
            ),
            Error::NamespaceDoesNotExist(id) => write!(f, "The namespace with ID {} does not exist", id.0),
            Error::NumberOfQueueEntriesLessThanTwo(entries) => write!(f,
                "The number of queue entries ({entries}) must not be smaller than 2."
            ),
            Error::NumberOfQueueEntriesMoreThanMaximum(entries, maximum) => write!(f,
                "The number of queue entries ({entries:X}) must not be bigger than \
                the maximum number of supported queue entries ({maximum})."
            ),
            Error::MaximumNumberOfQueuesReached => write!(f, "Maximum number of queues reached."),
            Error::IoQueuePairDoesNotExist(id) => write!(f, "The I/O queue pair with ID {} does not exist", id.0),
            Error::MemoryAccessOutOfBounds => write!(f, "Memory access out of bounds."),
            Error::UnixPciError(error) => write!(f, "{error}"),
            Error::VirtualAddressIsNotDwordAligned(address) => write!(f,
                "The virtual address {address:X} is not dword aligned."
            ),
            Error::VirtualAddressIsNotPageAligned(address) => write!(f,
                "The virtual address {address:X} is not page aligned."
            ),
            Error::BufferLengthBiggerThanMaximumTransferSize(buffer_length, maximum_transfer_size) => write!(f,
                "The buffer length ({buffer_length:X}) is bigger than the maximum transfer size ({maximum_transfer_size:X})."
            ),
            Error::BufferLengthNotAMultipleOfNamespaceBlockSize(buffer_length, block_size) => write!(f,
                "The buffer length ({buffer_length:X}) is not a multiple of the namespace block size ({block_size:X})."
            ),
            Error::IoCompletionQueueFailure(status) => write!(f,
                "I/O completion queue failed with status code 0x{:X} and type 0x{:X}",
                status & 0xFF,
                (status >> 8) & 0x7
            ),
            Error::SubmissionQueueFull => write!(f, "The submission queue is full."),
            Error::CompletionQueueCompletionFailure => write!(f,
                "The completion queue could not complete the command."
            ),
        }
    }
}

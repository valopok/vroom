use crate::cmd::NvmeCommand;
use crate::dma::{Allocator, Dma};
use crate::error::Error;
use crate::nvme::Namespace;
use crate::prp;
use crate::queues::*;
use ahash::RandomState;
use alloc::sync::Arc;
use hashbrown::HashMap;
use log::debug;

#[derive(Debug)]
pub(crate) struct AdminQueuePair {
    pub(crate) submission: SubmissionQueue,
    pub(crate) completion: CompletionQueue,
}

impl AdminQueuePair {
    pub(crate) fn submit_and_complete<F: FnOnce(u16, usize) -> NvmeCommand>(
        &mut self,
        cmd_init: F,
        buffer: &Dma<u8>,
        address: *mut u8,
        doorbell_stride: u16,
    ) -> Result<CompletionQueueEntry, Error> {
        let cid = self.submission.tail;
        let tail = self
            .submission
            .submit(cmd_init(cid as u16, buffer.physical_address() as usize));
        set_submission_queue_tail_doorbell(0, tail as u32, address, doorbell_stride);

        let (head, entry, _) = self.completion.complete_spin();
        set_completion_queue_head_doorbell(0, head as u32, address, doorbell_stride);
        let status = entry.status >> 1;
        if status != 0 {
            return Err(Error::IoCompletionQueueFailure(status));
        }
        Ok(entry)
    }
}

#[repr(C)]
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct IoQueuePairId(pub u16);

#[derive(Debug)]
pub struct IoQueuePair<A: Allocator> {
    pub(crate) id: IoQueuePairId,
    pub(crate) submission: SubmissionQueue,
    pub(crate) completion: CompletionQueue,
    pub(crate) page_size: usize,
    pub(crate) maximum_transfer_size: usize,
    pub(crate) allocator: Arc<A>,
    pub(crate) namespace: Namespace,
    pub(crate) device_address: usize,
    pub(crate) doorbell_stride: u16,
    pub(crate) prp_containers: HashMap<u16, prp::PrpContainer, RandomState>,
}

impl<A: Allocator> IoQueuePair<A> {
    pub fn id(&self) -> IoQueuePairId {
        self.id
    }

    pub fn allocate_buffer<T>(&self, number_of_elements: usize) -> Result<Dma<T>, Error> {
        if number_of_elements == 0 {
            return Err(Error::NumberOfElementsIsZero);
        }
        let size = number_of_elements * core::mem::size_of::<T>();
        debug!("Request buffer with {number_of_elements} elements and size 0x{size:X}.");
        let block_size = self.namespace.block_size;
        let next_multiple_of_block_size = size.next_multiple_of(block_size as usize);
        let number_of_elements = next_multiple_of_block_size / core::mem::size_of::<T>();
        debug!(
            "Allocate buffer with {number_of_elements} elements and size 0x{next_multiple_of_block_size:X}."
        );
        Dma::allocate(number_of_elements, self.page_size, self.allocator.as_ref())
    }

    pub fn deallocate_buffer<T>(&self, buffer: Dma<T>) -> Result<(), Error> {
        buffer.deallocate(self.allocator.as_ref())
    }

    /// Write the content of the provided `buffer` to the device at the `logical_block_address`.
    /// The `buffer` needs to be page aligned,
    /// its size must be a multiple of the name space block size and not exceed the maximum transfer size.
    pub fn write<T>(&mut self, buffer: &Dma<T>, logical_block_address: u64) -> Result<(), Error> {
        self.submit_write(buffer, logical_block_address)?;
        self.submission.head =
            self.complete_io()? as usize;
        Ok(())
    }

    /// Fill the provided `buffer` with data read from the device at the `logical_block_address`.
    /// The `buffer` needs to be page aligned,
    /// its size must be a multiple of the name space block size and not exceed the maximum transfer size.
    pub fn read<T>(
        &mut self,
        buffer: &mut Dma<T>,
        logical_block_address: u64,
    ) -> Result<(), Error> {
        self.submit_read(buffer, logical_block_address)?;
        self.submission.head =
            self.complete_io()? as usize;
        Ok(())
    }

    pub fn submit_read<T>(
        &mut self,
        buffer: &mut Dma<T>,
        logical_block_address: u64,
    ) -> Result<(), Error> {
        if buffer.size() > self.maximum_transfer_size {
            return Err(Error::BufferLengthBiggerThanMaximumTransferSize(
                buffer.size(),
                self.maximum_transfer_size,
            ));
        }
        if buffer.size() as u64 % self.namespace.block_size != 0 {
            return Err(Error::BufferLengthNotAMultipleOfNamespaceBlockSize(
                buffer.size(),
                self.namespace.block_size,
            ));
        }
        let prp_container = prp::allocate(buffer, self.page_size, self.allocator.as_ref())?;
        let prp_1 = prp_container.prp_1() as u64;
        let prp_2 = prp_container.prp_2().map(|prp_2| prp_2 as u64).unwrap_or(0);
        let blocks = buffer.size() as u64 / self.namespace.block_size;

        let command_id = self.submission.tail as u16;
        self.prp_containers
            .try_insert(command_id, prp_container)
            .map_err(|_| Error::PrpContainerAlreadyExists(command_id))?;

        let command = NvmeCommand::io_read(
            command_id,
            self.namespace.id.0,
            logical_block_address,
            blocks as u16 - 1,
            prp_1,
            prp_2,
        );

        let tail = self.submission.submit(command);
        set_submission_queue_tail_doorbell(
            self.id.0,
            tail as u32,
            self.device_address as *mut u8,
            self.doorbell_stride,
        );
        Ok(())
    }

    pub fn submit_write<T>(
        &mut self,
        buffer: &Dma<T>,
        logical_block_address: u64,
    ) -> Result<(), Error> {
        if buffer.size() > self.maximum_transfer_size {
            return Err(Error::BufferLengthBiggerThanMaximumTransferSize(
                buffer.size(),
                self.maximum_transfer_size,
            ));
        }
        if buffer.size() as u64 % self.namespace.block_size != 0 {
            return Err(Error::BufferLengthNotAMultipleOfNamespaceBlockSize(
                buffer.size(),
                self.namespace.block_size,
            ));
        }
        let prp_container = prp::allocate(buffer, self.page_size, self.allocator.as_ref())?;
        let prp_1 = prp_container.prp_1() as u64;
        let prp_2 = prp_container.prp_2().map(|prp_2| prp_2 as u64).unwrap_or(0);
        let blocks = buffer.size() as u64 / self.namespace.block_size;

        let command_id = self.submission.tail as u16;
        self.prp_containers
            .try_insert(command_id, prp_container)
            .map_err(|_| Error::PrpContainerAlreadyExists(command_id))?;

        let command = NvmeCommand::io_write(
            command_id,
            self.namespace.id.0,
            logical_block_address,
            blocks as u16 - 1,
            prp_1,
            prp_2,
        );

        let tail = self.submission.submit(command);
        set_submission_queue_tail_doorbell(
            self.id.0,
            tail as u32,
            self.device_address as *mut u8,
            self.doorbell_stride,
        );
        Ok(())
    }

    pub fn complete_io(&mut self) -> Result<u16, Error> {
        let (tail, completion_queue_entry, _) = self.completion.complete()?;
        unsafe {
            core::ptr::write_volatile(self.completion.doorbell as *mut u32, tail as u32);
        }
        self.submission.head = completion_queue_entry.sq_head as usize;
        let status = completion_queue_entry.status >> 1;
        if status != 0 {
            return Err(Error::IoCompletionQueueFailure(status));
        }
        let command_id = completion_queue_entry.command_id;
        let prp_container = self.prp_containers.remove(&command_id);
        if let Some(prp_container) = prp_container {
            prp::deallocate(prp_container, self.allocator.as_ref())?;
        }
        Ok(completion_queue_entry.sq_head)
    }
}

// SQyTDBL
fn set_submission_queue_tail_doorbell(
    queue_id: u16,
    value: u32,
    address: *mut u8,
    doorbell_stride: u16,
) {
    let tail_address = (address as usize
        + 0x1000
        + ((4 << doorbell_stride) * (2 * queue_id)) as usize) as *mut u32;
    unsafe { core::ptr::write_volatile(tail_address, value) };
}

// CQyHDBL
fn set_completion_queue_head_doorbell(
    queue_id: u16,
    value: u32,
    address: *mut u8,
    doorbell_stride: u16,
) {
    let head_address =
        (address as usize + 0x1000 + ((4 << doorbell_stride) * (2 * queue_id + 1)) as usize)
            as *mut u32;
    unsafe { core::ptr::write_volatile(head_address, value) };
}

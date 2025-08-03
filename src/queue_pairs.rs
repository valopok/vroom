use crate::cmd::NvmeCommand;
use crate::dma::{Allocator, Dma};
use crate::error::Error;
use crate::nvme::Namespace;
use crate::prp;
use crate::queues::*;
use alloc::sync::Arc;
use core::alloc::Layout;

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
            .submit(cmd_init(cid as u16, buffer.physical_address as usize));
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
}

impl<A: Allocator> IoQueuePair<A> {
    pub fn id(&self) -> IoQueuePairId {
        self.id
    }

    /// This method allocates an aligned buffer and copies the content from the provided `buffer`
    /// into it. The contents are then written to the device at the `logical_block_address`.
    pub fn write_copied(&mut self, buffer: &[u8], logical_block_address: u64) -> Result<(), Error> {
        if buffer.len() > self.maximum_transfer_size {
            return Err(Error::BufferLengthBiggerThanMaximumTransferSize(
                buffer.len(),
                self.maximum_transfer_size,
            ));
        }
        let layout = Layout::from_size_align(buffer.len(), self.page_size)
            .map_err(|error| Error::Layout(error))?;
        let aligned_buffer = self
            .allocator
            .allocate::<u8>(layout)
            .map_err(|error| Error::Allocate(error))?;
        let aligned_buffer = unsafe {
            core::slice::from_raw_parts_mut(aligned_buffer as *mut u8, aligned_buffer.len())
        };
        aligned_buffer[0..buffer.len()].copy_from_slice(buffer);
        self.write(aligned_buffer, logical_block_address)
    }

    /// Write the content of the provided `aligned_buffer` to the device at the `logical_block_address`.
    /// The `aligned_buffer` needs to be page aligned and its size must not exceed the maximum transfer size.
    ///
    /// See core::alloc::Layout for creation of such a buffer.
    ///
    /// To use an unaligned buffer, use `write_copied()` instead.
    pub fn write(
        &mut self,
        aligned_buffer: &[u8],
        logical_block_address: u64,
    ) -> Result<(), Error> {
        if aligned_buffer.len() > self.maximum_transfer_size {
            return Err(Error::BufferLengthBiggerThanMaximumTransferSize(
                aligned_buffer.len(),
                self.maximum_transfer_size,
            ));
        }
        if aligned_buffer.len() as u64 % self.namespace.block_size != 0 {
            return Err(Error::BufferLengthNotAMultipleOfNamespaceBlockSize(
                aligned_buffer.len(),
                self.namespace.block_size,
            ));
        }

        let prp_container = prp::allocate(
            aligned_buffer.as_ptr(),
            aligned_buffer.len(),
            self.page_size,
            self.allocator.as_ref(),
        )?;

        let prp_1 = prp_container.prp_1() as u64;
        let prp_2 = prp_container.prp_2().map(|prp_2| prp_2 as u64).unwrap_or(0);
        let blocks = aligned_buffer.len() as u64 / self.namespace.block_size;

        let entry = NvmeCommand::io_write(
            self.submission.tail as u16,
            self.namespace.id.0,
            logical_block_address,
            blocks as u16 - 1,
            prp_1,
            prp_2,
        );

        let tail = self.submission.submit(entry);
        // TODO: stats.submissions += 1;
        set_submission_queue_tail_doorbell(
            self.id.0,
            tail as u32,
            self.device_address as *mut u8,
            self.doorbell_stride,
        );
        self.submission.head = self.complete_io(1)? as usize;

        prp::deallocate(prp_container, self.allocator.as_ref())?;
        Ok(())
    }

    /// This method allocates an aligned buffer and fills it with data read from the `device` at
    /// the given `logical_block_address`. The contents of the aligned buffer are then copied to
    /// the provided `buffer`.
    pub fn read_copied(
        &mut self,
        buffer: &mut [u8],
        logical_block_address: u64,
    ) -> Result<(), Error> {
        if buffer.len() > self.maximum_transfer_size {
            return Err(Error::BufferLengthBiggerThanMaximumTransferSize(
                buffer.len(),
                self.maximum_transfer_size,
            ));
        }
        let layout = Layout::from_size_align(buffer.len(), self.page_size)
            .map_err(|error| Error::Layout(error))?;
        let aligned_buffer = self
            .allocator
            .allocate::<u8>(layout)
            .map_err(|error| Error::Allocate(error))?;
        let aligned_buffer = unsafe {
            core::slice::from_raw_parts_mut(aligned_buffer as *mut u8, aligned_buffer.len())
        };
        self.read(aligned_buffer, logical_block_address)?;
        buffer.copy_from_slice(&aligned_buffer[0..buffer.len()]);
        Ok(())
    }

    /// Fill the provided `aligned_buffer` with data read from the device at the `logical_block_address`.
    /// The `aligned_buffer` needs to be page aligned and its size must not exceed the maximum transfer size.
    ///
    /// See core::alloc::Layout for creation of such a buffer.
    ///
    /// To use an unaligned buffer, use `read_copied()` instead.
    pub fn read(
        &mut self,
        aligned_buffer: &mut [u8],
        logical_block_address: u64,
    ) -> Result<(), Error> {
        if aligned_buffer.len() > self.maximum_transfer_size {
            return Err(Error::BufferLengthBiggerThanMaximumTransferSize(
                aligned_buffer.len(),
                self.maximum_transfer_size,
            ));
        }
        if aligned_buffer.len() as u64 % self.namespace.block_size != 0 {
            return Err(Error::BufferLengthNotAMultipleOfNamespaceBlockSize(
                aligned_buffer.len(),
                self.namespace.block_size,
            ));
        }

        let prp_container = prp::allocate(
            aligned_buffer.as_ptr(),
            aligned_buffer.len(),
            self.page_size,
            self.allocator.as_ref(),
        )?;

        let prp_1 = prp_container.prp_1() as u64;
        let prp_2 = prp_container.prp_2().map(|prp_2| prp_2 as u64).unwrap_or(0);
        let blocks = aligned_buffer.len() as u64 / self.namespace.block_size;

        let entry = NvmeCommand::io_read(
            self.submission.tail as u16,
            self.namespace.id.0,
            logical_block_address,
            blocks as u16 - 1,
            prp_1,
            prp_2,
        );

        let tail = self.submission.submit(entry);
        // TODO: stats.submissions += 1;
        set_submission_queue_tail_doorbell(
            self.id.0,
            tail as u32,
            self.device_address as *mut u8,
            self.doorbell_stride,
        );
        self.submission.head = self.complete_io(1)? as usize;

        prp::deallocate(prp_container, self.allocator.as_ref())?;
        Ok(())
    }

    fn complete_io(&mut self, n: usize) -> Result<u16, Error> {
        assert!(n > 0);
        let (tail, completion_queue_entry, _) = self.completion.complete_n(n);
        unsafe {
            core::ptr::write_volatile(self.completion.doorbell as *mut u32, tail as u32);
        }
        self.submission.head = completion_queue_entry.sq_head as usize;
        let status = completion_queue_entry.status >> 1;
        if status != 0 {
            return Err(Error::IoCompletionQueueFailure(status));
        }
        Ok(completion_queue_entry.sq_head)
    }

    // TODO: test
    pub fn quick_poll(&mut self) -> Result<(), Error> {
        let (tail, completion_queue_entry, _) = self.completion.complete()?;
        unsafe {
            core::ptr::write_volatile(self.completion.doorbell as *mut u32, tail as u32);
        }
        self.submission.head = completion_queue_entry.sq_head as usize;
        let status = completion_queue_entry.status >> 1;
        if status != 0 {
            return Err(Error::IoCompletionQueueFailure(status));
        }
        Ok(())
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

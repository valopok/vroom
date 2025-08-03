use crate::cmd::NvmeCommand;
use crate::dma::{Allocator, Dma};
use crate::error::Error;
use core::hint::spin_loop;

#[derive(Debug)]
pub(crate) struct SubmissionQueue {
    commands: Dma<NvmeCommand>,
    pub(crate) head: usize,
    pub(crate) tail: usize,
    len: usize,
    #[allow(dead_code)]
    pub(crate) doorbell: usize,
}

#[derive(Debug)]
pub(crate) struct CompletionQueue {
    commands: Dma<CompletionQueueEntry>,
    head: usize,
    phase: bool,
    len: usize,
    pub(crate) doorbell: usize,
}

/// NVMe specification 4.6 Completion queue entry
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default)]
#[repr(C, packed)]
pub(crate) struct CompletionQueueEntry {
    /// Command specific
    pub(crate) command_specific: u32,
    pub(crate) _reserved: u32,
    // Submission queue head
    pub(crate) sq_head: u16,
    // Submission queue ID
    pub(crate) sq_id: u16,
    pub(crate) command_id: u16,
    pub(crate) status: u16,
}

impl SubmissionQueue {
    pub(crate) fn new<A: Allocator>(
        number_of_queue_entries: usize,
        page_size: usize,
        doorbell: usize,
        allocator: &A,
    ) -> Result<Self, Error> {
        Ok(Self {
            commands: Dma::allocate(number_of_queue_entries, page_size, allocator)?,
            head: 0,
            tail: 0,
            len: number_of_queue_entries,
            doorbell,
        })
    }

    #[allow(dead_code)]
    pub(crate) fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    #[allow(dead_code)]
    pub(crate) fn is_full(&self) -> bool {
        self.head == (self.tail + 1) % self.len
    }

    #[allow(dead_code)]
    pub(crate) fn submit_checked(&mut self, entry: NvmeCommand) -> Result<usize, Error> {
        if self.is_full() {
            Err(Error::SubmissionQueueFull)
        } else {
            Ok(self.submit(entry))
        }
    }

    #[inline(always)]
    pub(crate) fn submit(&mut self, entry: NvmeCommand) -> usize {
        // println!("SUBMISSION ENTRY: {:?}", entry);
        self.commands[self.tail] = entry;

        self.tail = (self.tail + 1) % self.len;
        self.tail
    }

    pub(crate) fn get_addr(&self) -> usize {
        self.commands.physical_address as usize
    }
}

impl CompletionQueue {
    pub(crate) fn new<A: Allocator>(
        number_of_queue_entries: usize,
        page_size: usize,
        doorbell: usize,
        allocator: &A,
    ) -> Result<Self, Error> {
        Ok(Self {
            commands: Dma::allocate(number_of_queue_entries, page_size, allocator)?,
            head: 0,
            phase: true,
            len: number_of_queue_entries,
            doorbell,
        })
    }

    #[inline(always)]
    pub(crate) fn complete(&mut self) -> Result<(usize, CompletionQueueEntry, usize), Error> {
        let entry = &self.commands[self.head];

        if ((entry.status & 1) == 1) == self.phase {
            let prev = self.head;
            self.head = (self.head + 1) % self.len;
            if self.head == 0 {
                self.phase = !self.phase;
            }
            Ok((self.head, *entry, prev))
        } else {
            Err(Error::CompletionQueueCompletionFailure)
        }
    }

    #[inline(always)]
    pub(crate) fn complete_n(&mut self, commands: usize) -> (usize, CompletionQueueEntry, usize) {
        let prev = self.head;
        self.head += commands - 1;
        if self.head >= self.len {
            self.phase = !self.phase;
        }
        self.head %= self.len;

        let (head, entry, _) = self.complete_spin();
        (head, entry, prev)
    }

    #[inline(always)]
    pub(crate) fn complete_spin(&mut self) -> (usize, CompletionQueueEntry, usize) {
        loop {
            if let Ok(val) = self.complete() {
                return val;
            }
            spin_loop();
        }
    }

    pub(crate) fn get_addr(&self) -> usize {
        self.commands.physical_address as usize
    }
}

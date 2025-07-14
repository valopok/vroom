use crate::cmd::{FeatureIdentifier, IdentifyNamespace, NvmeCommand, Select};
use crate::dma::{Allocator, Dma};
#[cfg(feature = "std")]
use crate::pci;
use crate::queue_pairs::{AdminQueuePair, IoQueuePair, IoQueuePairId};
use crate::queues::*;
use ahash::RandomState;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::error::Error;
use core::hint::spin_loop;
use hashbrown::HashMap;
use log::debug;

#[derive(Debug)]
pub struct ControllerInformation {
    pub pci_vendor_id: u16,
    pub pci_subsystem_vendor_id: u16,
    pub serial_number: String,
    pub model_number: String,
    pub firmware_revision: String,
    pub minimum_memory_page_size: u64,
    pub maximum_memory_page_size: u64,
    pub memory_page_size: usize,
    pub maximum_number_of_io_queue_pairs: u16,
    pub maximum_queue_entries_supported: u32,
    pub maximum_transfer_size: usize,
    pub controller_id: u16,
    pub version: u32,
}

#[derive(Debug)]
pub struct NvmeDevice<A> {
    allocator: Arc<A>,
    address: *mut u8,
    length: usize,
    doorbell_stride: u16,
    admin_queue_pair: AdminQueuePair,
    io_queue_pair_ids: Vec<IoQueuePairId>,
    information: ControllerInformation,
    namespaces: HashMap<u32, NvmeNamespace, RandomState>,
    buffer: Dma<u8>,
}

unsafe impl<A> Send for NvmeDevice<A> {}
unsafe impl<A> Sync for NvmeDevice<A> {}

impl<A: Allocator> NvmeDevice<A> {
    #[cfg(feature = "std")]
    pub fn from_pci_address(
        pci_address: &str,
        page_size: usize,
        allocator: A,
    ) -> Result<Self, Box<dyn Error>> {
        let mut vendor_file =
            pci::open_resource_readonly(pci_address, "vendor").expect("wrong pci address");
        let mut device_file =
            pci::open_resource_readonly(pci_address, "device").expect("wrong pci address");
        let mut config_file =
            pci::open_resource_readonly(pci_address, "config").expect("wrong pci address");

        let _vendor_id = pci::read_hex(&mut vendor_file)?;
        let _device_id = pci::read_hex(&mut device_file)?;
        let class_id = pci::read_io32(&mut config_file, 8)? >> 16;

        // 0x01 -> mass storage device class id
        // 0x08 -> nvme subclass
        if class_id != 0x0108 {
            return Err(format!("device {pci_address} is not a block device").into());
        }

        let (address, length) = pci::mmap_resource(pci_address)?;
        NvmeDevice::new(address, length, page_size, allocator)
    }

    pub fn new(
        address: *mut u8,
        length: usize,
        page_size: usize,
        allocator: A,
    ) -> Result<Self, Box<dyn Error>> {
        #[cfg(feature = "std")]
        env_logger::init();
        // TODO: follow Memory-based Controller Initialization (PCIe) from the NVMe specification
        debug!("Get capabilities");
        let cap = get_register_64(NvmeRegs64::CAP, address, length)?;
        let maximum_queue_entries_supported = (cap & 0xFFFF) as u32 + 1; // MQES (converted)
        let _contiguous_queues_required = ((cap >> 16) & 0b1) == 1; // CQR
        let _weighted_round_robin_with_urgent_priority_class = ((cap >> 17) & 0b1) == 1; // AMS: WRRUPC
        let _vendor_specific_ams = ((cap >> 18) & 0b1) == 1; // AMS: VS
        let _timeout_milliseconds = ((cap >> 24) & 0b1111_1111) as u32 * 500; // TO (converted)
        let doorbell_stride = ((cap >> 32) & 0b1111) as u16; // DSTRD
        let _nvm_subsystem_reset_supported = ((cap >> 36) & 0b1) == 1; // NSSRS
        let nvm_command_set_support = ((cap >> 37) & 0b1) == 1; // CSS: NCSS
        let _io_command_set_support = ((cap >> 43) & 0b1) == 1; // CSS: I/OCSS
        let _no_io_command_set_support = ((cap >> 44) & 0b1) == 1; // CSS: NOI/OCSS
        let _boot_partition_support = ((cap >> 45) & 0b1) == 1; // BPS
        let _controller_power_scope = ((cap >> 46) & 0b11) as u8; // CPS
        let minimum_memory_page_size = 1u64 << (((cap >> 48) & 0b1111) + 12); // MPSMIN (converted)
        let maximum_memory_page_size = 1u64 << (((cap >> 52) & 0b1111) + 12); // MPSMAX (converted)
        let _persistend_memory_region_supported = ((cap >> 56) & 0b1) == 1; // PMRS
        let _controller_memory_buffer_supported = ((cap >> 57) & 0b1) == 1; // CMBS
        let _nvm_subsystem_shutdown_supported = ((cap >> 58) & 0b1) == 1; // NSSS
        let _controller_ready_with_media_support = ((cap >> 59) & 0b1) == 1; // CRMS: CRIMS
        let _controller_ready_independent_of_media_support = ((cap >> 60) & 0b1) == 1; // CRMS: CRWMS
        let _nvm_subsystem_shutdown_enhancements_supported = ((cap >> 61) & 0b1) == 1; // NSSES

        if maximum_queue_entries_supported == 1 {
            return Err(
                "The value of \"Maximum Queue Entries Supported (MQES)\" in the
                capabilities register (CAP) is invalidly set to 0."
                    .into(),
            );
        }
        if !nvm_command_set_support {
            return Err("The NVM command set is not supported.".into());
        }
        if minimum_memory_page_size > maximum_memory_page_size {
            return Err(
                "The value of \"Memory Page Size Minimum (MPSMIN)\" is bigger than
                 the value of \"Memory Page Size Maximum (MPSMAX)\" in the capabilities register (CAP)." .into(),
            );
        }

        let ps_4_kibi_byte = 2usize.pow(12); // the lowest minimum page size
        let ps_128_mebi_byte = 2usize.pow(28); // the highest maximum page size
        if page_size < ps_4_kibi_byte {
            return Err(
                "The page size is less than the lowest minimum page size of 4 KiB (2^12 B).".into(),
            );
        }
        if page_size > ps_128_mebi_byte {
            return Err(
                "The page size is more than the highest maximum page size of 128 MiB (2^28 B)."
                    .into(),
            );
        }
        if (page_size as u64) < minimum_memory_page_size {
            return Err("The page size used is smaller than the minimum memory page size of the controller.".into());
        }
        if page_size as u64 > maximum_memory_page_size {
            return Err(
                "The page size used is bigger than the maximum memory page size of the controller."
                    .into(),
            );
        }
        if page_size.count_ones() != 1 {
            return Err("The page size is not a power of two.".into());
        }

        debug!("Disable controller");
        let mut cc = get_register_32(NvmeRegs32::CC, address, length)?;
        cc &= 0xFFFF_FFFE; // Set Enable (EN) to 0 to disable the controller.
        set_register_32(NvmeRegs32::CC, cc, address, length)?;

        // Wait for "not ready" signal
        loop {
            let csts = get_register_32(NvmeRegs32::CSTS, address, length)?;
            if csts & 1 == 1 {
                spin_loop();
            } else {
                break;
            }
        }

        debug!("Configure admin queues");
        let admin_sq = SubmissionQueue::new(MAX_SUB_QUEUE_ENTRIES, page_size, 0, &allocator)?;
        let admin_cq = CompletionQueue::new(MAX_SUB_QUEUE_ENTRIES, page_size, 0, &allocator)?;
        set_register_64(NvmeRegs64::ASQ, admin_sq.get_addr() as u64, address, length)?;
        set_register_64(NvmeRegs64::ACQ, admin_cq.get_addr() as u64, address, length)?;
        let aqa = (MAX_SUB_QUEUE_ENTRIES as u32 - 1) << 16 | (MAX_SUB_QUEUE_ENTRIES as u32 - 1);
        set_register_32(NvmeRegs32::AQA, aqa, address, length)?;
        let mut admin_queue_pair = AdminQueuePair {
            submission: admin_sq,
            completion: admin_cq,
        };

        debug!("Set controller configuration");
        let enable = 0b1; // EN
        let reserved_1 = 0b000 << 1;
        let io_command_set_selected = 0b000 << 4; // CSS TODO
        let memory_page_size = ((page_size.ilog2() - 12) & 0b1111) << 7; // MPS
        let arbitration_mechanism_selected = 0b000 << 11; // AMS TODO
        let shutdown_notification = 0b00 << 14; // SHN
        let io_submission_queue_entry_size = 6 << 16; // I/OSQES (2^n) TODO
        let io_completion_queue_entry_size = 4 << 20; // I/OCQES (2^n) TODO
        let controller_ready_independent_of_media_enable = 0b0 << 24; // CRIME TODO
        let reserved_2 = 0b000_0000 << 25;
        let cc = enable
            | reserved_1
            | io_command_set_selected
            | memory_page_size
            | arbitration_mechanism_selected
            | shutdown_notification
            | io_submission_queue_entry_size
            | io_completion_queue_entry_size
            | controller_ready_independent_of_media_enable
            | reserved_2;
        set_register_32(NvmeRegs32::CC, cc, address, length)?;

        debug!("Enable controller");
        // Wait for "ready" signal
        loop {
            let csts = get_register_32(NvmeRegs32::CSTS, address, length)?;
            if csts & 1 == 0 {
                spin_loop();
            } else {
                break;
            }
        }

        debug!("Allocate buffer");
        let buffer = Dma::allocate(page_size, page_size, &allocator)?;

        debug!("Identify controller");
        admin_queue_pair.submit_and_complete(
            NvmeCommand::identify_controller,
            &buffer,
            address,
            doorbell_stride,
        )?;
        fn read_c_string_from_slice(slice: &[u8]) -> String {
            let mut string = String::new();
            for &byte in slice {
                if byte == 0 {
                    break;
                }
                string.push(byte as char);
            }
            string.trim().to_string()
        }
        let pci_vendor_id = ((buffer[1] as u16) << 8) | buffer[0] as u16; // VID
        let pci_subsystem_vendor_id = ((buffer[3] as u16) << 8) | buffer[2] as u16; // SSVID
        let serial_number = read_c_string_from_slice(&buffer[4..=23]); // SN
        let model_number = read_c_string_from_slice(&buffer[24..=63]); // MN
        let firmware_revision = read_c_string_from_slice(&buffer[64..=71]); // FR
        let maximum_data_transfer_size = 1usize << buffer[77]; // MDTS (converted)
        let controller_id = ((buffer[79] as u16) << 8) | buffer[78] as u16; // CNTLID
        let version = ((buffer[83] as u32) << 24)
            | ((buffer[82] as u32) << 16)
            | ((buffer[81] as u32) << 8)
            | buffer[80] as u32; // VER
        let controller_type = buffer[111]; // CNTRLTYPE

        if controller_type != 1 {
            let type_name = match controller_type {
                0 => "not reported",
                2 => "discovery controller",
                3 => "administrative controller",
                _ => "unknown",
            };
            return Err(format!(
                "The controller type is not \"I/O controller\" but instead \"{type_name}\".",
            )
            .into());
        }
        let maximum_transfer_size = minimum_memory_page_size as usize * maximum_data_transfer_size;

        debug!("Get features");
        let completion_queue_entry = admin_queue_pair.submit_and_complete(
            |command_id, address| {
                NvmeCommand::get_features(
                    command_id,
                    address,
                    FeatureIdentifier::NumberOfQueues,
                    Select::Current,
                )
            },
            &buffer,
            address,
            doorbell_stride,
        )?;
        let dword_0 = completion_queue_entry.command_specific;
        // Not adding one to these, despite them being 0's based values,
        // because the admin queue pair is excluded.
        let number_of_io_submission_queues_allocated = dword_0 as u16;
        let number_of_io_completion_queues_allocated = (dword_0 >> 16) as u16;
        debug!(
            "Number of io submission queues allocated: {number_of_io_submission_queues_allocated}"
        );
        debug!(
            "Number of io completion queues allocated: {number_of_io_completion_queues_allocated}"
        );
        let maximum_number_of_io_queue_pairs =
            number_of_io_submission_queues_allocated.min(number_of_io_completion_queues_allocated);

        let information = ControllerInformation {
            pci_vendor_id,
            pci_subsystem_vendor_id,
            serial_number,
            model_number,
            firmware_revision,
            minimum_memory_page_size,
            maximum_memory_page_size,
            memory_page_size: page_size,
            maximum_number_of_io_queue_pairs,
            maximum_queue_entries_supported,
            maximum_transfer_size,
            controller_id,
            version,
        };
        debug!("{information:?}");

        debug!("Identify active namespace IDs");
        // Identify active namespace IDs
        admin_queue_pair.submit_and_complete(
            |c_id, address| NvmeCommand::identify_namespace_list(c_id, address, 0),
            &buffer,
            address,
            doorbell_stride,
        )?;
        let buffer_as_u32: &[u32] = unsafe {
            core::slice::from_raw_parts(buffer.virtual_address as *const u32, buffer.size / 4)
        };
        let namespace_ids = buffer_as_u32
            .iter()
            .copied()
            .take_while(|&id| id != 0)
            .collect::<Vec<u32>>();
        debug!("{namespace_ids:?}");

        debug!("Identify individual namespaces");
        // Identify individual namespaces
        let mut namespaces = HashMap::with_hasher(RandomState::with_seeds(0, 0, 0, 0));
        for namespace_id in namespace_ids {
            admin_queue_pair.submit_and_complete(
                |c_id, address| NvmeCommand::identify_namespace(c_id, address, namespace_id),
                &buffer,
                address,
                doorbell_stride,
            )?;

            let namespace_data: IdentifyNamespace =
                unsafe { (*(buffer.virtual_address as *const IdentifyNamespace)).clone() };

            // figure out block size
            let flba_index = (namespace_data.formatted_lba_size & 0xF) as usize;
            let flba_data = (namespace_data.lba_formats_list[flba_index] >> 16) & 0xFF;
            let block_size = if !(9..32).contains(&flba_data) {
                0
            } else {
                1 << flba_data
            };

            // TODO: check metadata?
            let namespace = NvmeNamespace {
                id: namespace_id,
                blocks: namespace_data.namespace_capacity,
                block_size,
            };
            debug!("{namespace:?}");
            namespaces.insert(namespace_id, namespace);
        }

        Ok(Self {
            allocator: Arc::new(allocator),
            address,
            doorbell_stride,
            length,
            admin_queue_pair,
            io_queue_pair_ids: Vec::new(),
            buffer,
            information,
            namespaces,
        })
    }

    pub fn controller_information(&self) -> &ControllerInformation {
        &self.information
    }

    pub fn namespace_ids(&self) -> Vec<u32> {
        self.namespaces.keys().copied().collect()
    }

    /// Create a pair consisting of 1 submission and 1 completion queue.
    pub fn create_io_queue_pair(
        &mut self,
        namespace_id: u32,
        number_of_queue_entries: u32,
    ) -> Result<IoQueuePair<A>, Box<dyn Error>> {
        let namespace = *self.namespaces.get(&namespace_id).ok_or(format!(
            "The namespace with ID {namespace_id} does not exist."
        ))?;

        // Simple way to avoid collisions while reusing some previously deleted keys.
        let mut index_option = None;
        for i in 1..self.information.maximum_number_of_io_queue_pairs {
            if !self.io_queue_pair_ids.contains(&IoQueuePairId(i)) {
                index_option = Some(IoQueuePairId(i));
                break;
            }
        }
        let queue_id = index_option.ok_or("Maximum number of queues reached")?;

        debug!("Requesting I/O queue pair with ID {}", queue_id.0);

        let offset = 0x1000 + ((4 << self.doorbell_stride) * (2 * queue_id.0 + 1) as usize);
        assert!(
            offset <= self.length - 4,
            "SQ doorbell offset out of bounds"
        );

        let dbl = self.address as usize + offset;
        let completion_queue = CompletionQueue::new(
            number_of_queue_entries as usize,
            self.information.memory_page_size,
            dbl,
            self.allocator.as_ref(),
        )?;
        self.submit_and_complete_admin(|c_id, _| {
            NvmeCommand::create_io_completion_queue(
                c_id,
                queue_id.0,
                completion_queue.get_addr(),
                (number_of_queue_entries - 1) as u16,
            )
        })?;

        let dbl = self.address as usize
            + 0x1000
            + ((4 << self.doorbell_stride) * (2 * queue_id.0) as usize);
        let submission_queue = SubmissionQueue::new(
            number_of_queue_entries as usize,
            self.information.memory_page_size,
            dbl,
            self.allocator.as_ref(),
        )?;
        self.submit_and_complete_admin(|c_id, _| {
            NvmeCommand::create_io_submission_queue(
                c_id,
                queue_id.0,
                submission_queue.get_addr(),
                (number_of_queue_entries - 1) as u16,
                queue_id.0,
            )
        })?;

        let io_queue_pair = IoQueuePair {
            id: queue_id,
            submission: submission_queue,
            completion: completion_queue,
            page_size: self.information.memory_page_size,
            maximum_transfer_size: self.information.maximum_transfer_size,
            allocator: self.allocator.clone(),
            namespace,
            device_address: self.address,
            doorbell_stride: self.doorbell_stride,
        };
        self.io_queue_pair_ids.push(queue_id);
        Ok(io_queue_pair)
    }

    pub fn delete_io_queue_pair(
        &mut self,
        queue_pair: IoQueuePair<A>,
    ) -> Result<(), Box<dyn Error>> {
        debug!("Deleting I/O queue pair with ID {}", queue_pair.id.0);
        let index = self
            .io_queue_pair_ids
            .iter()
            .position(|id| id == &queue_pair.id)
            .ok_or(format!(
                "The I/O queue pair with ID {} does not exist",
                queue_pair.id.0
            ))?;
        self.io_queue_pair_ids.remove(index);
        self.submit_and_complete_admin(|c_id, _| {
            NvmeCommand::delete_io_submission_queue(c_id, queue_pair.id.0)
        })?;
        self.submit_and_complete_admin(|c_id, _| {
            NvmeCommand::delete_io_completion_queue(c_id, queue_pair.id.0)
        })?;
        Ok(())
    }

    // TODO: test
    pub fn clear_namespace(&mut self, ns_id: Option<u32>) -> Result<(), Box<dyn Error>> {
        let ns_id = if let Some(ns_id) = ns_id {
            assert!(self.namespaces.contains_key(&ns_id));
            ns_id
        } else {
            0xFFFF_FFFF
        };
        self.admin_queue_pair.submit_and_complete(
            |c_id, _| NvmeCommand::format_nvm(c_id, ns_id),
            &self.buffer,
            self.address,
            self.doorbell_stride,
        )?;
        Ok(())
    }

    // TODO: deallocate all prp lists of all IO queues and the device buffer
    pub fn shutdown(self) -> Result<(), Box<dyn Error>> {
        todo!()
    }

    fn submit_and_complete_admin<F: FnOnce(u16, usize) -> NvmeCommand>(
        &mut self,
        cmd_init: F,
    ) -> Result<CompletionQueueEntry, Box<dyn Error>> {
        self.admin_queue_pair.submit_and_complete(
            cmd_init,
            &self.buffer,
            self.address,
            self.doorbell_stride,
        )
    }
}

/// Gets the value of the register at `address` + `register`.
/// Returns an error if `address` + `register` does not belong to mapped memory.
fn get_register_32(
    register: NvmeRegs32,
    address: *mut u8,
    length: usize,
) -> Result<u32, Box<dyn Error>> {
    if register as usize > length - 4 {
        return Err("Memory access out of bounds.".into());
    }
    let value =
        unsafe { core::ptr::read_volatile((address as usize + register as usize) as *mut u32) };
    Ok(value)
}

/// Gets the value of the register at `address` + `register`.
/// Returns an error if `address` + `register` does not belong to mapped memory.
fn get_register_64(
    register: NvmeRegs64,
    address: *mut u8,
    length: usize,
) -> Result<u64, Box<dyn Error>> {
    if register as usize > length - 8 {
        return Err("Memory access out of bounds.".into());
    }
    let value =
        unsafe { core::ptr::read_volatile((address as usize + register as usize) as *mut u64) };
    Ok(value)
}

/// Sets the register at `address` + `register` to `value`.
/// Returns an error if `address` + `register` does not belong to mapped memory.
fn set_register_32(
    register: NvmeRegs32,
    value: u32,
    address: *mut u8,
    length: usize,
) -> Result<(), Box<dyn Error>> {
    if register as usize > length - 4 {
        return Err("Memory access out of bounds.".into());
    }
    unsafe {
        core::ptr::write_volatile((address as usize + register as usize) as *mut u32, value);
    }
    Ok(())
}

/// Sets the register at `address` + `register` to `value`.
/// Returns an error if `address` + `register` does not belong to mapped memory.
fn set_register_64(
    register: NvmeRegs64,
    value: u64,
    address: *mut u8,
    length: usize,
) -> Result<(), Box<dyn Error>> {
    if register as usize > length - 8 {
        return Err("Memory access out of bounds.".into());
    }
    unsafe {
        core::ptr::write_volatile((address as usize + register as usize) as *mut u64, value);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct NvmeNamespace {
    pub id: u32,
    pub blocks: u64,
    pub block_size: u64,
}

// clippy doesnt like this
#[allow(unused, clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy)]
pub(crate) enum NvmeRegs32 {
    VS = 0x8,        // Version
    INTMS = 0xC,     // Interrupt Mask Set
    INTMC = 0x10,    // Interrupt Mask Clear
    CC = 0x14,       // Controller Configuration
    CSTS = 0x1C,     // Controller Status
    NSSR = 0x20,     // NVM Subsystem Reset
    AQA = 0x24,      // Admin Queue Attributes
    CMBLOC = 0x38,   // Contoller Memory Buffer Location
    CMBSZ = 0x3C,    // Controller Memory Buffer Size
    BPINFO = 0x40,   // Boot Partition Info
    BPRSEL = 0x44,   // Boot Partition Read Select
    BPMBL = 0x48,    // Bood Partition Memory Location
    CMBSTS = 0x58,   // Controller Memory Buffer Status
    PMRCAP = 0xE00,  // PMem Capabilities
    PMRCTL = 0xE04,  // PMem Region Control
    PMRSTS = 0xE08,  // PMem Region Status
    PMREBS = 0xE0C,  // PMem Elasticity Buffer Size
    PMRSWTP = 0xE10, // PMem Sustained Write Throughput
}

#[allow(unused, clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy)]
pub(crate) enum NvmeRegs64 {
    CAP = 0x0,      // Controller Capabilities
    ASQ = 0x28,     // Admin Submission Queue Base Address
    ACQ = 0x30,     // Admin Completion Queue Base Address
    CMBMSC = 0x50,  // Controller Memory Buffer Space Control
    PMRMSC = 0xE14, // Persistent Memory Buffer Space Control
}

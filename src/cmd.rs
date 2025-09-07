/// NVMe Spec 4.2
/// Submission queue entry
#[derive(Clone, Copy, Debug, Default)]
#[repr(C, packed)]
pub(crate) struct NvmeCommand {
    pub(crate) opcode: u8,
    /// Flags; FUSE (2 bits) | Reserved (4 bits) | PSDT (2 bits)
    pub(crate) flags: u8,
    pub(crate) command_id: u16,
    pub(crate) namespace_id: u32,
    pub(crate) _reserved: u64,
    pub(crate) metadata_pointer: u64,
    pub(crate) data_pointer: [u64; 2],
    /// Command dword 10
    pub(crate) cdw10: u32,
    /// Command dword 11
    pub(crate) cdw11: u32,
    /// Command dword 12
    pub(crate) cdw12: u32,
    /// Command dword 13
    pub(crate) cdw13: u32,
    /// Command dword 14
    pub(crate) cdw14: u32,
    /// Command dword 15
    pub(crate) cdw15: u32,
}

impl NvmeCommand {
    pub(crate) fn create_io_completion_queue(
        command_id: u16,
        queue_id: u16,
        data_pointer: usize,
        size: u16,
    ) -> Self {
        Self {
            opcode: 5,
            flags: 0,
            command_id,
            namespace_id: 0,
            _reserved: 0,
            metadata_pointer: 0,
            data_pointer: [data_pointer as u64, 0],
            cdw10: ((size as u32) << 16) | (queue_id as u32),
            cdw11: 1, // Physically Contiguous
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    pub(crate) fn create_io_submission_queue(
        command_id: u16,
        submission_queue_id: u16,
        data_pointer: usize,
        size: u16,
        completion_queue_id: u16,
    ) -> Self {
        Self {
            opcode: 1,
            flags: 0,
            command_id,
            namespace_id: 0,
            _reserved: 0,
            metadata_pointer: 0,
            data_pointer: [data_pointer as u64, 0],
            cdw10: ((size as u32) << 16) | (submission_queue_id as u32),
            cdw11: ((completion_queue_id as u32) << 16) | 1, /* Physically Contiguous */
            //TODO: QPRIO
            cdw12: 0, //TODO: NVMSETID
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    pub(crate) fn delete_io_submission_queue(command_id: u16, queue_id: u16) -> Self {
        Self {
            opcode: 0,
            command_id,
            cdw10: queue_id as u32,
            ..Default::default()
        }
    }

    pub(crate) fn delete_io_completion_queue(command_id: u16, queue_id: u16) -> Self {
        Self {
            opcode: 4,
            command_id,
            cdw10: queue_id as u32,
            ..Default::default()
        }
    }

    pub(crate) fn identify_namespace(
        command_id: u16,
        data_pointer: usize,
        namespace_id: u32,
    ) -> Self {
        Self {
            opcode: 6,
            flags: 0,
            command_id,
            namespace_id,
            _reserved: 0,
            metadata_pointer: 0,
            data_pointer: [data_pointer as u64, 0],
            cdw10: 0,
            cdw11: 0,
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    pub(crate) fn identify_controller(command_id: u16, data_pointer: usize) -> Self {
        Self {
            opcode: 6,
            flags: 0,
            command_id,
            namespace_id: 0,
            _reserved: 0,
            metadata_pointer: 0,
            data_pointer: [data_pointer as u64, 0],
            cdw10: 1,
            cdw11: 0,
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    pub(crate) fn identify_namespace_list(command_id: u16, data_pointer: usize, base: u32) -> Self {
        Self {
            opcode: 6,
            flags: 0,
            command_id,
            namespace_id: base,
            _reserved: 0,
            metadata_pointer: 0,
            data_pointer: [data_pointer as u64, 0],
            cdw10: 2,
            cdw11: 0,
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    pub(crate) fn get_features(
        _command_id: u16,
        data_pointer: usize,
        feature_id: FeatureIdentifier,
        select: Select,
    ) -> Self {
        Self {
            opcode: 0xA,
            data_pointer: [data_pointer as u64, 0],
            cdw10: ((select as u32) << 11) | feature_id as u32,
            ..Default::default()
        }
    }

    pub(crate) fn io_read(
        command_id: u16,
        namespace_id: u32,
        logical_block_address: u64,
        number_of_blocks: u16,
        prp_1: u64,
        prp_2: u64,
    ) -> Self {
        Self {
            opcode: 2,
            flags: 0,
            command_id,
            namespace_id,
            _reserved: 0,
            metadata_pointer: 0,
            data_pointer: [prp_1, prp_2],
            cdw10: logical_block_address as u32,
            cdw11: (logical_block_address >> 32) as u32,
            cdw12: number_of_blocks as u32,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    pub(crate) fn io_write(
        command_id: u16,
        namespace_id: u32,
        logical_block_address: u64,
        number_of_blocks: u16,
        prp_1: u64,
        prp_2: u64,
    ) -> Self {
        Self {
            opcode: 1,
            flags: 0,
            command_id,
            namespace_id,
            _reserved: 0,
            metadata_pointer: 0,
            data_pointer: [prp_1, prp_2],
            cdw10: logical_block_address as u32,
            cdw11: (logical_block_address >> 32) as u32,
            cdw12: number_of_blocks as u32,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    pub(crate) fn format_nvm(command_id: u16, namespace_id: u32) -> Self {
        Self {
            opcode: 0x80,
            flags: 0,
            command_id,
            namespace_id,
            _reserved: 0,
            metadata_pointer: 0,
            data_pointer: [0, 0],
            cdw10: 1 << 9,
            // TODO: dealloc and prinfo bits
            cdw11: 0,
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn async_event_req(command_id: u16) -> Self {
        Self {
            opcode: 0xC,
            flags: 0,
            command_id,
            namespace_id: 0,
            _reserved: 0,
            metadata_pointer: 0,
            data_pointer: [0, 0],
            cdw10: 0,
            cdw11: 0,
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn get_log_page(
        command_id: u16,
        numd: u32,
        ptr0: u64,
        ptr1: u64,
        lid: u8,
        lpid: u16,
    ) -> Self {
        Self {
            command_id,
            data_pointer: [ptr0, ptr1],
            cdw10: (numd << 16) | lid as u32,
            cdw11: ((lpid as u32) << 16) | numd >> 16,
            ..Self::default()
        }
    }

    #[allow(dead_code)]
    // not supported by samsung
    pub(crate) fn write_zeroes(
        command_id: u16,
        namespace_id: u32,
        slba: u64,
        nlb: u16,
        deac: bool,
    ) -> Self {
        Self {
            opcode: 8,
            flags: 0,
            command_id,
            namespace_id,
            _reserved: 0,
            metadata_pointer: 0,
            data_pointer: [0, 0],
            cdw10: slba as u32,
            // TODO: dealloc and prinfo bits
            cdw11: (slba >> 32) as u32,
            cdw12: ((deac as u32) << 25) | nlb as u32,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }
}

#[allow(dead_code)]
/// SEL
#[derive(Debug, Clone, Copy)]
pub(crate) enum Select {
    Current = 0b000,
    Default = 0b001,
    Saved = 0b010,
    SupportedCapabilites = 0b011,
}

#[allow(dead_code)]
/// FID
#[derive(Debug, Clone, Copy)]
pub(crate) enum FeatureIdentifier {
    Arbitration = 0x1,
    PowerManagement = 0x2,
    TemperatureThreshold = 0x4,
    VolatileWriteCache = 0x6,
    NumberOfQueues = 0x7,
    InterruptCoalescing = 0x08,
    InterruptVectorConfiguration = 0x09,
    AsynchronousEventConfiguration = 0x0B,
    AutonomousPowerStateTransition = 0x0C,
    HostMemoryBuffer = 0x0D,
    Timestamp = 0x0E,
    KeepAliveTimer = 0x0F,
    HostControlledThermalManagement = 0x10,
    NonOperationalPowerStateConfig = 0x11,
    ReadRecoveryLevelConfig = 0x12,
    PredictableLatencyModeConfig = 0x13,
    PredictableLatencyModeWindow = 0x14,
    HostBehaviorSupport = 0x16,
    SanitizeConfig = 0x17,
    EnduranceGroupEventConfiguration = 0x18,
    IOCommandSetProfile = 0x19,
    SpinupControl = 0x1A,
    PowerLossSignalingConfig = 0x1B,
    FlexibleDataPlacement = 0x1D,
    FlexibleDataPlacementEvents = 0x1E,
    NamespaceAdminLabel = 0x1F,
    ControllerDataQueue = 0x21,
    EmbeddedManagementControllerAddress = 0x78,
    HostManagementAgentAddress = 0x79,
    EnhancedControllerMetadata1 = 0x7D,
    ControllerMetadata1 = 0x7E,
    NamespaceMetadata1 = 0x7F,
    SoftwareProgressMarker = 0x80,
    HostIdentifier = 0x81,
    ReservationNotificationMask = 0x82,
    ReservationPersistence = 0x83,
    NamespaceWriteProtectionConfig = 0x84,
    BootPartitionWriteProtectionConfi = 0x85,
    // I/O Command Set specific features
}

#[repr(C, packed)]
#[derive(Debug, Clone)]
pub(crate) struct IdentifyNamespace {
    pub(crate) namespace_size: u64,                          // NSZE
    pub(crate) namespace_capacity: u64,                      // NCAP
    pub(crate) namespace_uitilization: u64,                  // NUSE
    pub(crate) namespace_features: u8,                       // NSFEAT
    pub(crate) number_of_lba_formats: u8,                    // NLBAF
    pub(crate) formatted_lba_size: u8,                       // FLBAS
    pub(crate) metadata_capabilites: u8,                     // MC
    pub(crate) end_to_end_data_protection_capabilites: u8,   // DPC
    pub(crate) end_to_end_data_protection_type_settings: u8, // DPS
    pub(crate) namespace_multi_path_io_and_namespace_sharing_capabilites: u8, // NMIC
    pub(crate) reservation_capabilities: u8,                 // RESCAP
    pub(crate) format_progress_indicator: u8,                // FPI
    pub(crate) deallocate_logical_block_features: u8,        // DLFEAT
    pub(crate) namespace_atomic_write_unit_normal: u16,      // NAWUN
    pub(crate) namespace_atomic_write_unit_power_fail: u16,  // NAWUPF
    pub(crate) namespace_atomic_compare_and_write_unit: u16, // NACWU
    pub(crate) namespace_atomic_boundary_size_normal: u16,   // NABSN
    pub(crate) namespace_atomic_boundary_offset: u16,        // NABO
    pub(crate) namespace_atomic_boundary_size_power_fail: u16, // NABSPF
    pub(crate) namespace_optimal_io_boundary: u16,           // NOIOB
    pub(crate) nvm_capacity: u128,                           // NVMCAP
    pub(crate) namespace_preferred_write_granularity: u16,   // NPWG
    pub(crate) namespace_preferred_write_alignment: u16,     // NPWA
    pub(crate) namespace_preferred_dallocate_granularity: u16, // NPDG
    pub(crate) namespace_preferred_dallocate_alignment: u16, // NPDA
    pub(crate) namespace_optimal_write_size: u16,            // NOWS
    pub(crate) maximum_single_source_range_length: u16,      // MSSRL
    pub(crate) maximum_copy_length: u32,                     // MCL
    pub(crate) maximum_source_range_count: u8,               // MSRC
    pub(crate) _reserved_1: [u8; 11],                        // (reserved)
    pub(crate) ana_group_identifier: u32,                    // ANAGRPID
    pub(crate) _reserved_2: [u8; 3],                         // (reserved)
    pub(crate) namespace_attributes: u8,                     // NSATTR
    pub(crate) nvm_set_identifier: u16,                      // NVMSETID
    pub(crate) endurance_group_identifier: u16,              // ENDGID
    pub(crate) namespace_globally_unique_identifier: [u8; 16], // NGUID
    pub(crate) ieee_extended_unique_identifier: u64,         // EUI64
    pub(crate) lba_formats_list: [u32; 64],                  // LBAF0, LBAF1, ... LBAF63
    pub(crate) vendor_specific: [u8; 3712],
}

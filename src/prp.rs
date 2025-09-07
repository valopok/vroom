use crate::dma::Allocator;
use crate::dma::Dma;
use crate::error::Error;
use alloc::vec::Vec;

// The physical region page list consists of multiple PRP entries.
// Each entry consists of an address to a region of physical memory and
// an offset inside of that region (packed into a u64).

/// The number of PRP entries needed depends on how many bytes need to be stored.
/// If one entry suffices, the physical address in that entry is stored in the `One` variant.
/// If two entries suffice, the physical addressess in those entries are stored in the `Two` variant.
/// If more than two entries are needed, the physical address in the first entry and the pointer to
/// the PRP lists are stored in the `Multiple` variant.
#[derive(Debug)]
pub(crate) enum PrpContainer {
    One(usize),                     // Address of PRP1
    Two(usize, usize),           // Address of PRP1 and PRP2
    Multiple(usize, Vec<Dma<u64>>), // Address of PRP1 and PRP list
}

impl PrpContainer {
    pub(crate) fn prp_1(&self) -> *mut u64 {
        match self {
            PrpContainer::One(prp_1) => *prp_1 as *mut u64,
            PrpContainer::Two(prp_1, _) => *prp_1 as *mut u64,
            PrpContainer::Multiple(prp_1, _) => *prp_1 as *mut u64,
        }
    }

    pub(crate) fn prp_2(&self) -> Option<*mut u64> {
        match self {
            PrpContainer::One(_) => None,
            PrpContainer::Two(_, prp_2) => Some(*prp_2 as *mut u64),
            PrpContainer::Multiple(_, prp_lists) => Some(prp_lists[0].physical_address()),
        }
    }
}

pub(crate) fn allocate<A: Allocator, T>(
    buffer: &Dma<T>,
    page_size: usize,
    allocator: &A,
) -> Result<PrpContainer, Error> {
    if (buffer.virtual_address() as usize & 0b0111) != 0 {
        return Err(Error::VirtualAddressIsNotDwordAligned(
            buffer.virtual_address() as usize,
        ));
    }
    let prp_1 = buffer.physical_address() as *mut u64;
    let needed_number_of_pages =
        ((buffer.virtual_address() as usize & (page_size - 1)) + buffer.size()).div_ceil(page_size);
    if needed_number_of_pages == 1 {
        return Ok(PrpContainer::One(prp_1 as usize));
    }
    if (buffer.virtual_address() as usize & (page_size - 1)) != 0 {
        return Err(Error::VirtualAddressIsNotPageAligned(
            buffer.virtual_address() as usize,
        ));
    }
    // add one page size to the virtual address of PRP1 to get the virtual address of PRP2
    let prp_2 = allocator
        .translate_virtual_to_physical(unsafe { buffer.virtual_address().add(page_size) })
        .map_err(Error::TranslateVirtualToPhysical)? as *mut u64;
    if needed_number_of_pages == 2 {
        return Ok(PrpContainer::Two(prp_1 as usize, prp_2 as usize));
    }

    let prp_entries_per_page = page_size / core::mem::size_of::<u64>();
    // subtracting 1 from the needed number of pages, because PRP1 points to the first needed page
    // subtracting 1 from the PRP entries per page, because one entry is needed as a pointer to the next PRP list
    let needed_number_of_prp_lists =
        (needed_number_of_pages - 1).div_ceil(prp_entries_per_page - 1);

    let mut prp_lists: Vec<Dma<u64>> = Vec::with_capacity(needed_number_of_prp_lists);
    for _ in 0..needed_number_of_prp_lists {
        prp_lists.push(Dma::allocate(prp_entries_per_page, page_size, allocator)?);
    }

    for i in 0..needed_number_of_prp_lists {
        // last entry is needed as a pointer to the next PRP list
        for j in 0..prp_entries_per_page - 1 {
            let offset = (i * (prp_entries_per_page - 1) + j) * page_size;
            prp_lists[i][j] = unsafe { prp_2.add(offset) } as u64;
        }
        // last list should not point to another list
        if i < needed_number_of_prp_lists - 1 {
            prp_lists[i][prp_entries_per_page - 1] = prp_lists[i + 1].physical_address() as u64;
        }
    }

    Ok(PrpContainer::Multiple(prp_1 as usize, prp_lists))
}

pub(crate) fn deallocate<A: Allocator>(
    prp_container: PrpContainer,
    allocator: &A,
) -> Result<(), Error> {
    if let PrpContainer::Multiple(_, prp_lists) = prp_container {
        for prp_list in prp_lists {
            prp_list.deallocate(allocator)?;
        }
    }
    Ok(())
}

use crate::dma::Allocator;
use std::boxed::Box;
use std::error::Error;
use std::format;
use std::io::{self, Read, Seek};
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{fs, mem, process, ptr};

const HUGE_PAGE_BITS: u32 = 21;
pub const HUGE_PAGE_SIZE: usize = 1 << HUGE_PAGE_BITS;

static HUGE_PAGE_ID: AtomicUsize = AtomicUsize::new(0);

pub struct HugePageAllocator;

impl Allocator for HugePageAllocator {
    fn allocate<T>(&self, layout: core::alloc::Layout) -> Result<*mut [T], Box<dyn Error>> {
        let size = layout.size();
        let size = if size % HUGE_PAGE_SIZE != 0 {
            ((size >> HUGE_PAGE_BITS) + 1) << HUGE_PAGE_BITS
        } else {
            size
        };

        let id = HUGE_PAGE_ID.fetch_add(1, Ordering::SeqCst);
        let path = format!("/mnt/huge/nvme-{}-{}", process::id(), id);

        match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path.clone())
        {
            Ok(f) => {
                let ptr = unsafe {
                    libc::mmap(
                        ptr::null_mut(),
                        size,
                        libc::PROT_READ | libc::PROT_WRITE,
                        libc::MAP_SHARED | libc::MAP_HUGETLB,
                        // libc::MAP_SHARED,
                        f.as_raw_fd(),
                        0,
                    )
                };
                if ptr == libc::MAP_FAILED {
                    Err("failed to mmap huge page - are huge pages enabled and free?".into())
                } else if unsafe { libc::mlock(ptr, size) } == 0 {
                    let slice = core::ptr::slice_from_raw_parts_mut(ptr, size);
                    Ok(slice as *mut [T])
                } else {
                    Err("failed to memory lock huge page".into())
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::NotFound => Err(Box::new(io::Error::new(
                e.kind(),
                format!("huge page {path} could not be created - huge pages enabled?"),
            ))),
            Err(e) => Err(Box::new(e)),
        }
    }
    fn deallocate<T>(&self, _slice: *mut [T]) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
    fn translate_virtual_to_physical<T>(
        &self,
        virtual_address: *const T,
    ) -> Result<*const T, Box<dyn Error>> {
        let pagesize = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;

        let mut file = fs::OpenOptions::new()
            .read(true)
            .open("/proc/self/pagemap")?;

        file.seek(io::SeekFrom::Start(
            (virtual_address as usize / pagesize * mem::size_of::<usize>()) as u64,
        ))?;

        let mut buffer = [0; mem::size_of::<usize>()];
        file.read_exact(&mut buffer)?;

        let phys = usize::from_ne_bytes(buffer);
        Ok(
            ((phys & 0x007F_FFFF_FFFF_FFFF) * pagesize + virtual_address as usize % pagesize)
                as *const T,
        )
    }
}

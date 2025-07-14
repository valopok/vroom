use alloc::boxed::Box;
use core::error::Error;
use core::ops::{Deref, DerefMut, Index, IndexMut, Range, RangeFull, RangeInclusive, RangeTo};
use core::slice;

pub trait Allocator {
    fn allocate<T>(
        &self,
        layout: core::alloc::Layout,
    ) -> Result<*mut [T], Box<dyn core::error::Error>>;
    fn deallocate<T>(&self, slice: *mut [T]) -> Result<(), Box<dyn core::error::Error>>;
    fn translate_virtual_to_physical<T>(
        &self,
        virtual_address: *const T,
    ) -> Result<*const T, Box<dyn core::error::Error>>;
}

#[derive(Debug)]
pub(crate) struct Dma<T> {
    pub(crate) virtual_address: *mut T,
    pub(crate) physical_address: *mut T,
    pub(crate) size: usize,
}

impl<T> Dma<T> {
    pub(crate) fn allocate<A: Allocator>(
        number_of_elements: usize,
        page_size: usize,
        allocator: &A,
    ) -> Result<Dma<T>, Box<dyn Error>> {
        let layout = core::alloc::Layout::from_size_align(
            core::mem::size_of::<T>() * number_of_elements,
            page_size,
        )?;
        let virtual_address = allocator.allocate::<T>(layout)?;
        let physical_address =
            allocator.translate_virtual_to_physical(virtual_address as *mut T)?;
        let dma = Dma {
            virtual_address: virtual_address as *mut T,
            physical_address: physical_address as *mut T,
            size: number_of_elements,
        };
        Ok(dma)
    }

    pub(crate) fn deallocate<A: Allocator>(self, allocator: &A) -> Result<(), Box<dyn Error>> {
        let slice = core::ptr::slice_from_raw_parts_mut(self.virtual_address, self.size);
        allocator.deallocate(slice)
    }
}

unsafe impl<T> Send for Dma<T> {}
unsafe impl<T> Sync for Dma<T> {}

impl<T> Deref for Dma<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.virtual_address }
    }
}

impl<T> DerefMut for Dma<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.virtual_address }
    }
}

impl<T> Index<usize> for Dma<T> {
    type Output = T;
    fn index(&self, index: usize) -> &Self::Output {
        assert!(index < self.size, "Index out of bounds");
        unsafe { &*self.virtual_address.add(index) }
    }
}

impl<T> IndexMut<usize> for Dma<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        assert!(index < self.size, "Index out of bounds");
        unsafe {
            &mut *self
                .virtual_address
                .add(index)
        }
    }
}

impl Index<Range<usize>> for Dma<u8> {
    type Output = [u8];
    fn index(&self, index: Range<usize>) -> &Self::Output {
        assert!(index.end <= self.size, "Index out of bounds");
        unsafe {
            slice::from_raw_parts(
                self.virtual_address.add(index.start),
                index.end - index.start,
            )
        }
    }
}

impl IndexMut<Range<usize>> for Dma<u8> {
    fn index_mut(&mut self, index: Range<usize>) -> &mut Self::Output {
        assert!(index.end <= self.size, "Index out of bounds");
        unsafe {
            slice::from_raw_parts_mut(
                self.virtual_address.add(index.start),
                index.end - index.start,
            )
        }
    }
}

impl Index<RangeTo<usize>> for Dma<u8> {
    type Output = [u8];
    fn index(&self, index: RangeTo<usize>) -> &Self::Output {
        &self[0..index.end]
    }
}

impl IndexMut<RangeTo<usize>> for Dma<u8> {
    fn index_mut(&mut self, index: RangeTo<usize>) -> &mut Self::Output {
        &mut self[0..index.end]
    }
}

impl Index<RangeInclusive<usize>> for Dma<u8> {
    type Output = [u8];
    fn index(&self, index: RangeInclusive<usize>) -> &Self::Output {
        &self[*index.start()..(*index.end() + 1)]
    }
}

impl IndexMut<RangeInclusive<usize>> for Dma<u8> {
    fn index_mut(&mut self, index: RangeInclusive<usize>) -> &mut Self::Output {
        &mut self[*index.start()..(*index.end() + 1)]
    }
}

impl Index<RangeFull> for Dma<u8> {
    type Output = [u8];
    fn index(&self, _: RangeFull) -> &Self::Output {
        &self[0..self.size]
    }
}

impl IndexMut<RangeFull> for Dma<u8> {
    fn index_mut(&mut self, _: RangeFull) -> &mut Self::Output {
        let len = self.size;
        &mut self[0..len]
    }
}

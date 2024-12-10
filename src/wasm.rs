// Contains portions of code from https://github.com/alexcrichton/dlmalloc-rs/tree/b8f84faa62d82afc33179b013eb21cbff6111e0d
// Dual-licensed under MIT and Apache 2.0
// See https://github.com/alexcrichton/dlmalloc-rs/tree/b8f84faa62d82afc33179b013eb21cbff6111e0d/LICENSE-APACHE and
// https://github.com/alexcrichton/dlmalloc-rs/tree/b8f84faa62d82afc33179b013eb21cbff6111e0d/LICENSE-MIT

use std::alloc::{GlobalAlloc, Layout};
use std::{mem, ptr};

use dlmalloc::Dlmalloc;

struct GlobalDlmalloc;

static mut DLMALLOC: Dlmalloc = Dlmalloc::new();

unsafe impl GlobalAlloc for GlobalDlmalloc {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _guard = lock();
        let dlmalloc = ptr::addr_of_mut!(DLMALLOC);
        (*dlmalloc).malloc(layout.size(), layout.align())
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let _guard = lock();
        let dlmalloc = ptr::addr_of_mut!(DLMALLOC);
        (*dlmalloc).free(ptr, layout.size(), layout.align())
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let _guard = lock();
        let dlmalloc = ptr::addr_of_mut!(DLMALLOC);
        (*dlmalloc).calloc(layout.size(), layout.align())
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _guard = lock();
        let dlmalloc = ptr::addr_of_mut!(DLMALLOC);
        (*dlmalloc).realloc(ptr, layout.size(), layout.align(), new_size)
    }
}

unsafe fn lock() -> impl Drop {
    // single threaded, no need!
    assert!(!cfg!(target_feature = "atomics"));

    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {}
    }

    Guard
}

// WARNING: Code below relies on memory allocator internals. This is incredibly hacky.
// Those are functions are provided for linked C libraries and they *WILL* break if you change the allocator
// or even if the allocator changes its internal implementation in the future.

#[global_allocator]
static ALLOC: GlobalDlmalloc = GlobalDlmalloc;

#[no_mangle]
pub unsafe extern "C" fn malloc(size: usize) -> *mut u8 {
    let _guard = lock();
    let dlmalloc = ptr::addr_of_mut!(DLMALLOC);
    (*dlmalloc).malloc(size, 4)
}

const PINUSE: usize = 1 << 0;
const CINUSE: usize = 1 << 1;
const FLAG4: usize = 1 << 2;
const INUSE: usize = PINUSE | CINUSE;
const FLAG_BITS: usize = PINUSE | CINUSE | FLAG4;

#[repr(C)]
struct Chunk {
    prev_foot: usize,
    head: usize,
    prev: *mut Chunk,
    next: *mut Chunk,
}

impl Chunk {
    unsafe fn from_mem(mem: *mut u8) -> *mut Chunk {
        mem.sub(2 * mem::size_of::<usize>()).cast()
    }

    const fn size(&self) -> usize {
        self.head & !FLAG_BITS
    }

    const fn get_min_overhead(&self) -> usize {
        if self.head & INUSE == 0 {
            2 * mem::size_of::<usize>()
        } else {
            mem::size_of::<usize>()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn free(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }

    let _guard = lock();
    let dlmalloc = ptr::addr_of_mut!(DLMALLOC);

    let chunk = Chunk::from_mem(ptr);
    let size = (*chunk).size() - (*chunk).get_min_overhead();

    (*dlmalloc).free(ptr, size, 0);
}

#[no_mangle]
pub unsafe extern "C" fn calloc(count: usize, size: usize) -> *mut u8 {
    let _guard = lock();
    let layout = Layout::from_size_align(size * count, 4).unwrap();
    let dlmalloc = ptr::addr_of_mut!(DLMALLOC);
    (*dlmalloc).calloc(layout.size(), layout.align())
}

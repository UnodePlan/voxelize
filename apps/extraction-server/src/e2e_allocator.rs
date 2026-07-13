use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

static LIVE_BYTES: AtomicU64 = AtomicU64::new(0);
static PEAK_BYTES: AtomicU64 = AtomicU64::new(0);
static LIVE_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static ALLOCATION_COUNT: AtomicU64 = AtomicU64::new(0);
static DEALLOCATION_COUNT: AtomicU64 = AtomicU64::new(0);
static REALLOCATION_COUNT: AtomicU64 = AtomicU64::new(0);

pub(crate) struct TrackingAllocator;

impl TrackingAllocator {
    pub(crate) const fn new() -> Self {
        Self
    }
}

// 只在 e2e-control 二进制启用；原子计数不分配内存，避免递归进入 allocator。
unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            record_allocation(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            record_allocation(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        LIVE_BYTES.fetch_sub(layout.size() as u64, Ordering::Relaxed);
        LIVE_ALLOCATIONS.fetch_sub(1, Ordering::Relaxed);
        DEALLOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let resized = unsafe { System.realloc(pointer, layout, new_size) };
        if !resized.is_null() {
            adjust_live_bytes(layout.size(), new_size);
            REALLOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        resized
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AllocatorSnapshot {
    pub(crate) live_bytes: u64,
    pub(crate) peak_bytes: u64,
    pub(crate) live_allocations: u64,
    pub(crate) allocation_count: u64,
    pub(crate) deallocation_count: u64,
    pub(crate) reallocation_count: u64,
}

pub(crate) fn snapshot() -> AllocatorSnapshot {
    AllocatorSnapshot {
        live_bytes: LIVE_BYTES.load(Ordering::Relaxed),
        peak_bytes: PEAK_BYTES.load(Ordering::Relaxed),
        live_allocations: LIVE_ALLOCATIONS.load(Ordering::Relaxed),
        allocation_count: ALLOCATION_COUNT.load(Ordering::Relaxed),
        deallocation_count: DEALLOCATION_COUNT.load(Ordering::Relaxed),
        reallocation_count: REALLOCATION_COUNT.load(Ordering::Relaxed),
    }
}

fn record_allocation(size: usize) {
    let live = LIVE_BYTES.fetch_add(size as u64, Ordering::Relaxed) + size as u64;
    PEAK_BYTES.fetch_max(live, Ordering::Relaxed);
    LIVE_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    ALLOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
}

fn adjust_live_bytes(old_size: usize, new_size: usize) {
    let live = if new_size >= old_size {
        LIVE_BYTES.fetch_add((new_size - old_size) as u64, Ordering::Relaxed)
            + (new_size - old_size) as u64
    } else {
        LIVE_BYTES.fetch_sub((old_size - new_size) as u64, Ordering::Relaxed)
            - (old_size - new_size) as u64
    };
    PEAK_BYTES.fetch_max(live, Ordering::Relaxed);
}

use std::collections::HashMap;

use crate::prelude::*;
use crate::renderer::BufferHandle;
use offset_allocator::{Allocation, Allocator};

/// Hard cap of one page. offset-allocator uses u32 units, and we use
/// stride-sized units — so the byte capacity of a single page is at most
/// `u32::MAX * stride`. This byte budget is the request we pass to the
/// buffer-creation callback; we leave slack for alignment padding.
pub const MAX_PAGE_BYTES: u32 = u32::MAX - 256;

/// First page size. Subsequent pages double until they hit `MAX_PAGE_BYTES`.
const INITIAL_PAGE_BYTES: u32 = 16 * 1024 * 1024;

const INDEX_STRIDE: u32 = 4; // u32 indices

/// Locates a mesh's vertex allocation: which layout set, which page in that set.
/// Meshes store this so draws know where their data lives.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct VertexPoolId {
    pub layout_index: u32,
    pub page_index: u32,
}

pub struct VertexPoolPage {
    pub buffer: BufferHandle,
    /// Allocator operates in units of `stride` bytes. This means
    /// `Allocation::offset` is directly the `base_vertex` for draw_indexed,
    /// and all byte offsets are naturally stride-aligned.
    pub allocator: Allocator,
    /// Capacity in vertex units (not bytes).
    pub capacity: u32,
}

pub struct VertexPoolSet {
    pub layout: VertexLayout,
    pub pages: Vec<VertexPoolPage>,
}

pub struct IndexPool {
    pub buffer: BufferHandle,
    /// Allocator in units of 4 bytes; `Allocation::offset` is `first_index`.
    pub allocator: Allocator,
    pub capacity: u32,
}

pub struct PoolManager {
    pub layouts: Vec<VertexPoolSet>,
    pub layout_lookup: HashMap<VertexLayout, u32>,
    pub index_pages: Vec<IndexPool>,
}

impl PoolManager {
    pub fn new() -> Self {
        Self {
            layouts: Vec::new(),
            layout_lookup: HashMap::new(),
            index_pages: Vec::new(),
        }
    }

    pub fn get_or_create_layout(&mut self, layout: &VertexLayout) -> u32 {
        if let Some(&idx) = self.layout_lookup.get(layout) {
            return idx;
        }
        let idx = self.layouts.len() as u32;
        self.layouts.push(VertexPoolSet {
            layout: layout.clone(),
            pages: Vec::new(),
        });
        self.layout_lookup.insert(layout.clone(), idx);
        idx
    }

    /// Allocate `count` vertices in the pool for `layout_index`. First tries
    /// existing pages; if none fit, calls `create_buffer` to back a new page.
    /// Returns (page_index, allocation). `allocation.offset` is the base vertex.
    pub fn alloc_vertices(
        &mut self,
        layout_index: u32,
        count: u32,
        stride: u32,
        create_buffer: &mut dyn FnMut(u32) -> BufferHandle,
    ) -> (u32, Allocation) {
        assert!(stride > 0, "vertex layout stride must be > 0");
        let set = &mut self.layouts[layout_index as usize];

        for (page_idx, page) in set.pages.iter_mut().enumerate() {
            if let Some(alloc) = page.allocator.allocate(count) {
                return (page_idx as u32, alloc);
            }
        }

        let needed_bytes = count.saturating_mul(stride);
        let prev_bytes = set
            .pages
            .last()
            .map(|p| p.capacity.saturating_mul(stride))
            .unwrap_or(0);
        let doubled = prev_bytes.saturating_mul(2).max(INITIAL_PAGE_BYTES);
        let target_bytes = doubled.max(needed_bytes).min(MAX_PAGE_BYTES);
        let cap_bytes = target_bytes - (target_bytes % stride);
        let cap_units = cap_bytes / stride;
        assert!(
            count <= cap_units,
            "single mesh too large for a page: requested {} vertices, page cap {}",
            count,
            cap_units
        );

        let buffer = create_buffer(cap_bytes);
        let mut allocator = Allocator::new(cap_units);
        let alloc = allocator
            .allocate(count)
            .expect("fresh allocator must satisfy request");
        set.pages.push(VertexPoolPage {
            buffer,
            allocator,
            capacity: cap_units,
        });
        ((set.pages.len() - 1) as u32, alloc)
    }

    pub fn alloc_indices(
        &mut self,
        count: u32,
        create_buffer: &mut dyn FnMut(u32) -> BufferHandle,
    ) -> (u32, Allocation) {
        for (page_idx, page) in self.index_pages.iter_mut().enumerate() {
            if let Some(alloc) = page.allocator.allocate(count) {
                return (page_idx as u32, alloc);
            }
        }

        let needed_bytes = count.saturating_mul(INDEX_STRIDE);
        let prev_bytes = self
            .index_pages
            .last()
            .map(|p| p.capacity.saturating_mul(INDEX_STRIDE))
            .unwrap_or(0);
        let doubled = prev_bytes.saturating_mul(2).max(INITIAL_PAGE_BYTES);
        let target_bytes = doubled.max(needed_bytes).min(MAX_PAGE_BYTES);
        let cap_bytes = target_bytes - (target_bytes % INDEX_STRIDE);
        let cap_units = cap_bytes / INDEX_STRIDE;
        assert!(
            count <= cap_units,
            "single mesh too large for an index page: requested {} indices, page cap {}",
            count,
            cap_units
        );

        let buffer = create_buffer(cap_bytes);
        let mut allocator = Allocator::new(cap_units);
        let alloc = allocator
            .allocate(count)
            .expect("fresh allocator must satisfy request");
        self.index_pages.push(IndexPool {
            buffer,
            allocator,
            capacity: cap_units,
        });
        ((self.index_pages.len() - 1) as u32, alloc)
    }

    pub fn free_vertices(&mut self, pool: VertexPoolId, allocation: Allocation) {
        let page = &mut self.layouts[pool.layout_index as usize].pages[pool.page_index as usize];
        page.allocator.free(allocation);
    }

    pub fn free_indices(&mut self, page_index: u32, allocation: Allocation) {
        self.index_pages[page_index as usize]
            .allocator
            .free(allocation);
    }

    pub fn vertex_buffer(&self, pool: VertexPoolId) -> BufferHandle {
        self.layouts[pool.layout_index as usize].pages[pool.page_index as usize].buffer
    }

    pub fn index_buffer(&self, page_index: u32) -> BufferHandle {
        self.index_pages[page_index as usize].buffer
    }
}

use std::{fs::File, os::fd::AsFd};

use memmap2::{MmapMut, MmapOptions};
use rustix::fs::{ftruncate, memfd_create, MemfdFlags};
use wayland_client::{
    protocol::{wl_buffer, wl_shm, wl_shm_pool},
    Proxy, QueueHandle,
};

use crate::{BackendError, Frame, PixelFormat, Result, SurfaceId};

const SLOT_COUNT: usize = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BufferKey {
    pub surface: SurfaceId,
    pub slot: usize,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BufferSpec {
    width: u32,
    height: u32,
    format: PixelFormat,
}

impl BufferSpec {
    fn from_frame(frame: Frame<'_>) -> Self {
        Self {
            width: frame.width,
            height: frame.height,
            format: frame.format,
        }
    }

    fn byte_len(self) -> Result<usize> {
        let pixels = u64::from(self.width)
            .checked_mul(u64::from(self.height))
            .and_then(|value| value.checked_mul(4))
            .ok_or_else(|| BackendError::InvalidFrame("buffer size overflow".to_owned()))?;
        if pixels > i32::MAX as u64 || self.width > i32::MAX as u32 || self.height > i32::MAX as u32
        {
            return Err(BackendError::InvalidFrame(
                "wl_shm buffer exceeds protocol i32 limits".to_owned(),
            ));
        }
        Ok(pixels as usize)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AllocationPlan {
    Reallocate,
    Reuse(usize),
    WouldBlock,
}

#[derive(Debug, Default)]
struct BufferLifecycle {
    spec: Option<BufferSpec>,
    generation: u64,
    busy: [bool; SLOT_COUNT],
}

impl BufferLifecycle {
    fn plan(&self, spec: BufferSpec) -> AllocationPlan {
        if self.spec != Some(spec) {
            return if self.busy.iter().any(|busy| *busy) {
                AllocationPlan::WouldBlock
            } else {
                AllocationPlan::Reallocate
            };
        }
        self.busy
            .iter()
            .position(|busy| !*busy)
            .map(AllocationPlan::Reuse)
            .unwrap_or(AllocationPlan::WouldBlock)
    }

    fn reallocated(&mut self, spec: BufferSpec) {
        self.spec = Some(spec);
        self.generation = self.generation.wrapping_add(1);
        self.busy = [false; SLOT_COUNT];
    }

    fn mark_busy(&mut self, slot: usize) {
        self.busy[slot] = true;
    }

    fn allocation_failed(&mut self) {
        self.spec = None;
        self.busy = [false; SLOT_COUNT];
    }

    fn release(&mut self, key: BufferKey) {
        if key.generation == self.generation && key.slot < SLOT_COUNT {
            self.busy[key.slot] = false;
        }
    }
}

struct Slot {
    _file: File,
    map: MmapMut,
    buffer: wl_buffer::WlBuffer,
}

pub(crate) struct ShmBuffers {
    lifecycle: BufferLifecycle,
    slots: Vec<Slot>,
}

impl ShmBuffers {
    pub(crate) fn new() -> Self {
        Self {
            lifecycle: BufferLifecycle::default(),
            slots: Vec::with_capacity(SLOT_COUNT),
        }
    }

    pub(crate) fn release(&mut self, key: BufferKey) {
        self.lifecycle.release(key);
    }

    pub(crate) fn acquire<State>(
        &mut self,
        surface: SurfaceId,
        frame: Frame<'_>,
        shm: &wl_shm::WlShm,
        qh: &QueueHandle<State>,
    ) -> Result<wl_buffer::WlBuffer>
    where
        State: wayland_client::Dispatch<wl_buffer::WlBuffer, BufferKey>
            + wayland_client::Dispatch<wl_shm_pool::WlShmPool, ()>
            + 'static,
    {
        validate_frame(frame)?;
        let spec = BufferSpec::from_frame(frame);
        let mut slot = match self.lifecycle.plan(spec) {
            AllocationPlan::Reuse(slot) => slot,
            AllocationPlan::WouldBlock => return Err(BackendError::WouldBlock),
            AllocationPlan::Reallocate => {
                self.destroy();
                self.lifecycle.reallocated(spec);
                self.allocate(surface, spec, shm, qh)?;
                0
            }
        };

        if self.lifecycle.busy[slot] {
            slot = self
                .lifecycle
                .busy
                .iter()
                .position(|busy| !*busy)
                .ok_or(BackendError::WouldBlock)?;
        }

        copy_frame(&mut self.slots[slot].map, frame)?;
        self.lifecycle.mark_busy(slot);
        Ok(self.slots[slot].buffer.clone())
    }

    fn allocate<State>(
        &mut self,
        surface: SurfaceId,
        spec: BufferSpec,
        shm: &wl_shm::WlShm,
        qh: &QueueHandle<State>,
    ) -> Result<()>
    where
        State: wayland_client::Dispatch<wl_buffer::WlBuffer, BufferKey>
            + wayland_client::Dispatch<wl_shm_pool::WlShmPool, ()>
            + 'static,
    {
        let byte_len = spec.byte_len()?;
        let stride = spec
            .width
            .checked_mul(4)
            .ok_or_else(|| BackendError::InvalidFrame("stride overflow".to_owned()))?
            as i32;

        let mut slots = Vec::with_capacity(SLOT_COUNT);
        for slot_index in 0..SLOT_COUNT {
            let allocated = (|| -> Result<Slot> {
                let fd = memfd_create("nuraloumi-shm", MemfdFlags::CLOEXEC)
                    .map_err(std::io::Error::from)?;
                ftruncate(&fd, byte_len as u64).map_err(std::io::Error::from)?;
                let file = File::from(fd);
                let map = unsafe { MmapOptions::new().len(byte_len).map_mut(&file)? };
                let pool = shm.create_pool(file.as_fd(), byte_len as i32, qh, ());
                let format = match spec.format {
                    PixelFormat::Argb8888 => wl_shm::Format::Argb8888,
                    PixelFormat::Xrgb8888 => wl_shm::Format::Xrgb8888,
                };
                let buffer = pool.create_buffer(
                    0,
                    spec.width as i32,
                    spec.height as i32,
                    stride,
                    format,
                    qh,
                    BufferKey {
                        surface,
                        slot: slot_index,
                        generation: self.lifecycle.generation,
                    },
                );
                pool.destroy();
                Ok(Slot {
                    _file: file,
                    map,
                    buffer,
                })
            })();

            match allocated {
                Ok(slot) => slots.push(slot),
                Err(error) => {
                    destroy_slots(&mut slots);
                    self.lifecycle.allocation_failed();
                    return Err(error);
                }
            }
        }
        self.slots = slots;
        Ok(())
    }

    pub(crate) fn destroy(&mut self) {
        destroy_slots(&mut self.slots);
    }
}

fn destroy_slots(slots: &mut Vec<Slot>) {
    for slot in slots.drain(..) {
        if slot.buffer.is_alive() {
            slot.buffer.destroy();
        }
    }
}

impl Drop for ShmBuffers {
    fn drop(&mut self) {
        self.destroy();
    }
}

fn validate_frame(frame: Frame<'_>) -> Result<()> {
    if frame.width == 0 || frame.height == 0 {
        return Err(BackendError::InvalidFrame(
            "width and height must be non-zero".to_owned(),
        ));
    }

    let row_bytes = (frame.width as usize)
        .checked_mul(frame.format.bytes_per_pixel())
        .ok_or_else(|| BackendError::InvalidFrame("row size overflow".to_owned()))?;
    if frame.stride < row_bytes {
        return Err(BackendError::InvalidFrame(format!(
            "stride {} is smaller than packed row size {row_bytes}",
            frame.stride
        )));
    }

    let required = frame
        .stride
        .checked_mul(frame.height as usize)
        .ok_or_else(|| BackendError::InvalidFrame("frame length overflow".to_owned()))?;
    if frame.pixels.len() < required {
        return Err(BackendError::InvalidFrame(format!(
            "pixel slice has {} bytes but frame requires at least {required}",
            frame.pixels.len()
        )));
    }

    Ok(())
}

fn copy_frame(target: &mut MmapMut, frame: Frame<'_>) -> Result<()> {
    let row_bytes = (frame.width as usize)
        .checked_mul(frame.format.bytes_per_pixel())
        .ok_or_else(|| BackendError::InvalidFrame("row size overflow".to_owned()))?;
    let packed_len = row_bytes
        .checked_mul(frame.height as usize)
        .ok_or_else(|| BackendError::InvalidFrame("packed frame length overflow".to_owned()))?;
    if target.len() != packed_len {
        return Err(BackendError::InvalidFrame(format!(
            "mapped buffer has {} bytes, expected {packed_len}",
            target.len()
        )));
    }

    if frame.stride == row_bytes {
        target.copy_from_slice(&frame.pixels[..packed_len]);
    } else {
        for row in 0..frame.height as usize {
            let source_start = row * frame.stride;
            let target_start = row * row_bytes;
            target[target_start..target_start + row_bytes]
                .copy_from_slice(&frame.pixels[source_start..source_start + row_bytes]);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(width: u32, height: u32) -> BufferSpec {
        BufferSpec {
            width,
            height,
            format: PixelFormat::Argb8888,
        }
    }

    #[test]
    fn lifecycle_never_allocates_past_two_busy_slots() {
        let mut lifecycle = BufferLifecycle::default();
        assert_eq!(lifecycle.plan(spec(100, 40)), AllocationPlan::Reallocate);
        lifecycle.reallocated(spec(100, 40));
        lifecycle.mark_busy(0);
        lifecycle.mark_busy(1);

        for width in 101..200 {
            assert_eq!(lifecycle.plan(spec(width, 40)), AllocationPlan::WouldBlock);
            assert_eq!(lifecycle.busy, [true, true]);
        }
    }

    #[test]
    fn resize_waits_until_all_old_buffers_are_released() {
        let mut lifecycle = BufferLifecycle::default();
        lifecycle.reallocated(spec(100, 40));
        lifecycle.mark_busy(0);
        assert_eq!(lifecycle.plan(spec(200, 40)), AllocationPlan::WouldBlock);
        lifecycle.release(BufferKey {
            surface: SurfaceId(1),
            slot: 0,
            generation: lifecycle.generation,
        });
        assert_eq!(lifecycle.plan(spec(200, 40)), AllocationPlan::Reallocate);
    }

    #[test]
    fn failed_allocation_resets_spec_for_clean_retry() {
        let mut lifecycle = BufferLifecycle::default();
        lifecycle.reallocated(spec(100, 40));
        lifecycle.mark_busy(0);
        lifecycle.allocation_failed();

        assert_eq!(lifecycle.spec, None);
        assert_eq!(lifecycle.busy, [false, false]);
        assert_eq!(lifecycle.plan(spec(100, 40)), AllocationPlan::Reallocate);
    }

    #[test]
    fn stale_release_cannot_free_new_generation_slot() {
        let mut lifecycle = BufferLifecycle::default();
        lifecycle.reallocated(spec(100, 40));
        let old_generation = lifecycle.generation;
        lifecycle.reallocated(spec(200, 40));
        lifecycle.mark_busy(0);
        lifecycle.release(BufferKey {
            surface: SurfaceId(1),
            slot: 0,
            generation: old_generation,
        });
        assert!(lifecycle.busy[0]);
    }
}

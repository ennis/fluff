use std::alloc::Layout;
use std::collections::BTreeSet;
use std::marker::PhantomData;
use std::{mem, ptr};
use std::ops::{Deref, DerefMut};
use zerocopy::IntoBytes;
use kyute_common::Color;
use crate::{CmdFillPathSolid, CmdKind, GradientStop, MAX_OFFSET, PathMoveTo, PathLineTo, PathQuadratic, PathCurveTo, PathCmd, PackF32, Offset24, Path, ColorIndex};

const MAX_ALIGN: usize = 4;

struct Buffer {
    data: Vec<u8>,
}

impl Buffer
{
    fn new() -> Self {
        Buffer { data: Vec::new() }
    }

    fn reserve_layout(&mut self, layout: Layout) -> usize
    {
        assert!(layout.align() <= MAX_ALIGN);
        let offset = self.data.len();
        let end_ptr = self.data.as_ptr().wrapping_add(offset);
        let align_offset = end_ptr.align_offset(layout.align());
        self.data.reserve(align_offset + layout.size());
        offset + align_offset
    }

    fn write<T: IntoBytes>(&mut self, data: &T) -> u32 {
        let p = self.reserve_layout(Layout::new::<T>());
        unsafe {
            let ptr = self.data.as_mut_ptr().wrapping_add(p) as *mut T;
            let sz = mem::size_of::<T>();
            ptr::copy_nonoverlapping(data.as_bytes().as_ptr(), ptr, sz);
            self.data.set_len(p + sz);
        }
        p as u32
    }

    fn write_tag_prefixed<T: IntoBytes>(&mut self, tag: u32, data: &T) -> u32 {
        let p = self.write(&tag);
        self.write(data);
        p
    }

    fn write_tag(&mut self, tag: u32) {
        self.write(&tag);
    }

    fn write_length_prefixed(&mut self, data: &[u8]) -> u32 {
        let p = self.write(&(data.len() as u32));
        self.data.extend_from_slice(data);
        p
    }
}


pub struct PathWriter<'a> {
    w: &'a mut BytecodeWriter,
    buf: Buffer,
}

impl<'a> PathWriter<'a> {
    pub fn move_to(&mut self, x: impl Into<PackF32>, y: impl Into<PackF32>) {
        self.buf.write_tag_prefixed(PathCmd::MoveTo as u32, &PathMoveTo { x: x.into(), y: y.into() });
    }

    pub fn line_to(&mut self, x: impl Into<PackF32>, y: impl Into<PackF32>) {
        self.buf.write_tag_prefixed(PathCmd::LineTo as u32, &PathLineTo { x: x.into(), y: y.into() });
    }

    pub fn quad_to(&mut self, x1: impl Into<PackF32>, y1: impl Into<PackF32>, x: impl Into<PackF32>, y: impl Into<PackF32>) {
        self.buf.write_tag_prefixed(PathCmd::Quadratic as u32, &PathQuadratic { x1: x1.into(), y1: y1.into(), x: x.into(), y: y.into() });
    }

    pub fn cubic_to(&mut self, x1: impl Into<PackF32>, y1: impl Into<PackF32>, x2: impl Into<PackF32>, y2: impl Into<PackF32>, x: impl Into<PackF32>, y: impl Into<PackF32>) {
        self.buf.write_tag_prefixed(PathCmd::CurveTo as u32, &PathCurveTo { x1: x1.into(), y1: y1.into(), x2: x2.into(), y2: y2.into(), x: x.into(), y: y.into() });
    }

    pub fn close(&mut self) {
        self.buf.write_tag(PathCmd::ClosePath as u32);
    }

    pub fn finish(&mut self) -> Offset24<Path> {
        let p = self.w.buf.write_length_prefixed(&self.buf.data[..]);
        Offset24::new(p)
    }
}

pub struct BytecodeWriter {
    buf: Buffer,
}

impl BytecodeWriter {
    pub fn record_path(&mut self, path: &[u8]) -> PathWriter {
        PathWriter { w: self, buf: Buffer::new() }
    }

    pub fn add_palette_entry(&mut self, color: Color) -> ColorIndex {
        let (r, g, b, a) = color.to_rgba_u8();
        let color = (r as u32) << 24 | (g as u32) << 16 | (b as u32) << 8 | a as u32;
    }
}


pub struct CmdListWriter<'a> {
    w: &'a mut BytecodeWriter,
    buf: Buffer,
}

impl<'a> Deref for CmdListWriter<'a> {
    type Target = BytecodeWriter;

    fn deref(&self) -> &Self::Target {
        self.w
    }
}

impl<'a> DerefMut for CmdListWriter<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.w
    }
}

impl<'a> CmdListWriter<'a> {
    pub fn fill_path_solid(&mut self, x: f32, y: f32, width: f32, height: f32, color: u32) {
        self.buf.write_tag_prefixed(CmdKind::FillPathSolid as u32, &CmdFillPathSolid { path: 0, color: ColorIndex() });
    }

    pub fn record_cmd_list(&mut self) -> CmdListWriter {
        CmdListWriter {
            w: self.w,
            buf: Buffer::new(),
        }
    }

    pub fn finish(&mut self) -> u32 {
        self.w.buf.write_length_prefixed(&self.buf.data)
    }
}


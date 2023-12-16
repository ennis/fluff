//! Houdini geometry (.geo) file parser.
//!
//! For now it only supports JSON-based `.geo` files. Binary files (`.bgeo`) are not supported.

mod error;
mod parser;

pub use error::Error;
use smol_str::SmolStr;
use std::{fs, path::Path};

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Copy, Clone, Debug)]
pub enum StorageKind {
    FpReal32,
    FpReal64,
    Int32,
    Int64,
}

#[derive(Clone, Debug)]
pub enum AttributeStorage {
    FpReal32(Vec<f32>),
    FpReal64(Vec<f64>),
    Int32(Vec<i32>),
    Int64(Vec<i64>),
}

/// Geometry attribute.
#[derive(Clone, Debug)]
pub struct Attribute {
    /// Name of the attribute.
    pub name: SmolStr,
    /// Number of elements per tuple.
    pub size: usize,
    /// Storage.
    pub storage: AttributeStorage,
}

impl Attribute {
    pub fn as_f32_slice(&self) -> Option<&[f32]> {
        match &self.storage {
            AttributeStorage::FpReal32(data) => Some(data),
            _ => None,
        }
    }

    pub fn as_i32_slice(&self) -> Option<&[i32]> {
        match &self.storage {
            AttributeStorage::Int32(data) => Some(data),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Topology {
    pub indices: Vec<u32>,
}

#[derive(Clone, Debug)]
pub enum Primitive {
    BezierRun(BezierRun),
}

/// The contents of a houdini geometry file.
#[derive(Clone, Debug, Default)]
pub struct Geo {
    pub point_count: usize,
    pub vertex_count: usize,
    pub primitive_count: usize,
    pub topology: Vec<u32>,
    pub point_attributes: Vec<Attribute>,
    pub primitive_attributes: Vec<Attribute>,
    pub primitives: Vec<Primitive>,
}

impl Geo {
    /// Find a point attribute by name.
    ///
    /// TODO version that returns a typed attribute?
    pub fn find_point_attribute(&self, name: &str) -> Option<&Attribute> {
        self.point_attributes.iter().find(|a| a.name == name)
    }
}

/// Bezier curve basis.
#[derive(Clone, Debug, Default)]
pub struct BezierBasis {
    pub order: u32,
    pub knots: Vec<f32>,
}

/// A geometry variable that can be either uniform over a sequence of elements,
/// or varying per element.
#[derive(Clone, Debug)]
pub enum PrimVar<T> {
    /// Same value for all elements in the sequence.
    Uniform(T),
    /// One value per element in the sequence.
    Varying(Vec<T>),
}

/// A run of bezier curves.
#[derive(Clone, Debug)]
pub struct BezierRun {
    /// Number of curves in the run.
    pub count: usize,
    /// Vertices of the control points.
    ///
    /// They are indices into the `topology` vector.
    /// They are usually `Varying`, because a bezier run with the same control points isn't very useful.
    pub vertices: PrimVar<Vec<i32>>,
    /// Whether the curve is closed.
    pub closed: PrimVar<bool>,
    /// Curve basis information.
    pub basis: PrimVar<BezierBasis>,
}

pub struct BezierRunIter<'a> {
    run: &'a BezierRun,
    index: usize,
}

impl<'a> Iterator for BezierRunIter<'a> {
    type Item = BezierRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.run.count {
            return None;
        }

        let vertices = match &self.run.vertices {
            PrimVar::Uniform(vertices) => vertices.as_slice(),
            PrimVar::Varying(vertices) => &vertices[self.index],
        };

        let closed = match &self.run.closed {
            PrimVar::Uniform(closed) => *closed,
            PrimVar::Varying(closed) => closed[self.index],
        };

        let basis = match &self.run.basis {
            PrimVar::Uniform(basis) => basis,
            PrimVar::Varying(basis) => &basis[self.index],
        };

        self.index += 1;

        Some(BezierRef {
            vertices,
            closed,
            basis,
        })
    }
}

impl BezierRun {
    pub fn iter(&self) -> BezierRunIter {
        BezierRunIter { run: self, index: 0 }
    }
}

/// Represents a bezier curve.
pub struct BezierRef<'a> {
    /// Vertices of the control points.
    ///
    /// They are indices into the `topology` vector.
    pub vertices: &'a [i32],
    /// Whether the curve is closed.
    pub closed: bool,
    /// Curve basis information.
    pub basis: &'a BezierBasis,
}

impl Default for BezierRun {
    fn default() -> Self {
        BezierRun {
            count: 0,
            vertices: PrimVar::Varying(vec![]),
            closed: PrimVar::Varying(vec![]),
            basis: PrimVar::Varying(vec![]),
        }
    }
}

impl Geo {
    pub fn load_json<P: AsRef<Path>>(path: P) -> Result<Geo, Error> {
        let data = fs::read_to_string(path)?;
        parser::parse_json(&data)
    }
}

#[cfg(test)]
mod test {
    use crate::Geo;

    #[test]
    fn compiles() {
        let path = "../../data/untitled3155.geo";
        let geo = Geo::load_json(path).unwrap();
        eprintln!("{:#?}", geo);
    }
}

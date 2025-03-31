use crate::error::Error;
use crate::property::{PropertyReader, TypedScalarPropertyReader};
use crate::{CompoundPropertyReader, Result, TimeSampling};
use glam::dvec3;
use std::mem::MaybeUninit;
use std::{iter, mem};

pub struct XForm {
    //this: CompoundPropertyReader,
    pub child_bounds: Option<TypedScalarPropertyReader<[f32; 6]>>,
    inherits: Option<TypedScalarPropertyReader<bool>>,
    ops: Vec<XFormOp>,
    vals: PropertyReader,
    sample_count: usize,
}

impl XForm {
    /// TODO: maybe acquire ownership of CompoundPropertyReader?
    pub fn new(parent_props: &CompoundPropertyReader, prop_name: &str) -> Result<Self> {
        let properties = parent_props.compound_property(prop_name)?;
        let child_bounds = TypedScalarPropertyReader::new(&properties, ".childBnds").ok();
        let inherits = TypedScalarPropertyReader::new(&properties, ".inherits").ok();

        let ops = if properties.has_property(".ops") {
            let ops_raw = TypedScalarPropertyReader::<[u8]>::new_array(&properties, ".ops")?.get(0)?;
            ops_raw
                .iter()
                .map(|&enc_op| match enc_op >> 4 {
                    op if op == XFormOp::Scale as u8 => Ok(XFormOp::Scale),
                    op if op == XFormOp::Translate as u8 => Ok(XFormOp::Translate),
                    op if op == XFormOp::Rotate as u8 => Ok(XFormOp::Rotate),
                    op if op == XFormOp::Matrix as u8 => Ok(XFormOp::Matrix),
                    op if op == XFormOp::RotateX as u8 => Ok(XFormOp::RotateX),
                    op if op == XFormOp::RotateY as u8 => Ok(XFormOp::RotateY),
                    op if op == XFormOp::RotateZ as u8 => Ok(XFormOp::RotateZ),
                    _ => return Err(Error::MalformedData),
                })
                .collect::<Result<Vec<_>>>()?
        } else {
            vec![]
        };

        /*let anim_chans = if properties.has_property(".animChans") {
            TypedScalarPropertyReader::<[u32]>::new_array(&properties, ".animChans")?.get(0)?
        } else {
            vec![]
        };*/

        let vals = properties.property(".vals")?;
        let sample_count = vals.sample_count();

        Ok(Self {
            child_bounds,
            inherits,
            ops,
            vals,
            sample_count,
        })
    }

    /// Returns the number of transform samples.
    pub fn sample_count(&self) -> usize {
        self.sample_count
    }

    /// Sample the transform at a given time.
    pub fn get(&self, sample_index: usize) -> Result<[[f64; 4]; 4]> {
        // read channel values
        let vals = match &self.vals {
            PropertyReader::Scalar(s) => {
                let extent = s.extent();
                let mut vals = vec![MaybeUninit::uninit(); extent];
                s.read_array_sample_into::<f64>(sample_index, &mut vals)?;
                // SAFETY: the values are initialized, or we returned an error already
                unsafe { mem::transmute(vals) }
            }
            PropertyReader::Array(a) => {
                let vals = a.read_sample::<f64>(sample_index)?;
                vals.values
            }
            PropertyReader::Compound(_) => {
                unreachable!()
            }
        };

        let mut ich = 0;
        let mut m = glam::DMat4::IDENTITY;
        for op in &self.ops {
            if ich + op.channel_count() > vals.len() {
                return Err(Error::MalformedData);
            }

            let ch = &vals[ich..ich + op.channel_count()];
            m = m * xform_op_to_matrix(*op, ch);
            // TODO apply matrix
            ich += op.channel_count();
        }

        Ok(m.to_cols_array_2d())
    }

    pub fn time_sampling(&self) -> &TimeSampling {
        if let Some(inherits) = &self.inherits {
            inherits.time_sampling()
        } else {
            // FIXME: what's the default?
            self.vals.time_sampling()
        }
    }

    /// Returns an iterator over all samples of the transform.
    pub fn samples(&self) -> impl Iterator<Item = (f64, [[f64; 4]; 4])> + '_ {
        let count = self.vals.sample_count();
        let time_sampling = self.time_sampling();
        let mut i = 0;
        iter::from_fn(move || {
            if i < count {
                let time = time_sampling
                    .get_sample_time(i)
                    .expect("Failed to read transform sample time");
                let value = self.get(i).expect("Failed to read transform sample");
                i += 1;
                Some((time, value))
            } else {
                None
            }
        })
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(u8)]
pub enum XFormOp {
    Scale = 0,
    Translate,
    Rotate,
    Matrix,
    RotateX,
    RotateY,
    RotateZ,
}

impl XFormOp {
    fn channel_count(&self) -> usize {
        match self {
            XFormOp::Scale => 3,
            XFormOp::Translate => 3,
            XFormOp::Rotate => 4,
            XFormOp::Matrix => 16,
            XFormOp::RotateX => 1,
            XFormOp::RotateY => 1,
            XFormOp::RotateZ => 1,
        }
    }
}

/// Builds a transform matrix from channel values according to a transform op type.
///
/// # Arguments
/// * `op` - the transform operation
/// * `ch` - data for the transform operation; it must contain enough elements for the operation (i.e. `op.channel_count()`).
fn xform_op_to_matrix(op: XFormOp, ch: &[f64]) -> glam::DMat4 {
    let m;
    match op {
        XFormOp::Scale => {
            m = glam::DMat4::from_scale(dvec3(ch[0], ch[1], ch[2]));
        }
        XFormOp::Translate => {
            m = glam::DMat4::from_translation(dvec3(ch[0], ch[1], ch[2]));
        }
        XFormOp::Rotate => {
            m = glam::DMat4::from_axis_angle(dvec3(ch[0], ch[1], ch[2]), ch[3]);
        }
        XFormOp::Matrix => {
            let cols: &[f64; 16] = ch.try_into().expect("Matrix must have 16 elements");
            m = glam::DMat4::from_cols_array(cols)
        }
        XFormOp::RotateX => {
            m = glam::DMat4::from_rotation_x(ch[0]);
        }
        XFormOp::RotateY => {
            m = glam::DMat4::from_rotation_y(ch[0]);
        }
        XFormOp::RotateZ => {
            m = glam::DMat4::from_rotation_z(ch[0]);
        }
    }
    m
}

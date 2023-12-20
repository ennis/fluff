mod binary;
mod json;

use crate::{Attribute, AttributeStorage, BezierBasis, BezierRun, Error, Error::Malformed, Geo, PrimVar, Primitive, StorageKind};
use json::ParserImpl;
use smol_str::SmolStr;

#[derive(PartialEq, Debug)]
enum Event {
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    BeginArray,
    EndArray,
    BeginMap,
    EndMap,
}

impl Event {
    fn as_integer(&self) -> Option<i64> {
        match self {
            Event::Integer(i) => Some(*i),
            Event::Float(f) => Some(*f as i64),
            _ => None,
        }
    }

    fn as_float(&self) -> Option<f64> {
        match self {
            Event::Integer(i) => Some(*i as f64),
            Event::Float(f) => Some(*f),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Event::String(s) => Some(s),
            _ => None,
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

impl StorageKind {
    fn parse(s: &str) -> Result<StorageKind, Error> {
        match s {
            "fpreal32" => Ok(StorageKind::FpReal32),
            "fpreal64" => Ok(StorageKind::FpReal64),
            "int32" => Ok(StorageKind::Int32),
            "int64" => Ok(StorageKind::Int64),
            _ => Err(Error::Malformed),
        }
    }
}

impl AttributeStorage {
    fn new(storage_kind: StorageKind) -> AttributeStorage {
        match storage_kind {
            StorageKind::FpReal32 => AttributeStorage::FpReal32(Vec::new()),
            StorageKind::FpReal64 => AttributeStorage::FpReal64(Vec::new()),
            StorageKind::Int32 => AttributeStorage::Int32(Vec::new()),
            StorageKind::Int64 => AttributeStorage::Int64(Vec::new()),
        }
    }

    fn read_element(&mut self, p: &mut ParserImpl) -> Result<(), Error> {
        //eprintln!("read_element");
        match p.next().ok_or(Error::EarlyEof)? {
            Event::Float(f) => match self {
                AttributeStorage::FpReal32(v) => v.push(f as f32),
                AttributeStorage::FpReal64(v) => v.push(f as f64),
                AttributeStorage::Int32(v) => v.push(f as i32),
                AttributeStorage::Int64(v) => v.push(f as i64),
            },
            Event::Integer(i) => match self {
                AttributeStorage::FpReal32(v) => v.push(i as f32),
                AttributeStorage::FpReal64(v) => v.push(i as f64),
                AttributeStorage::Int32(v) => v.push(i as i32),
                AttributeStorage::Int64(v) => v.push(i),
            },
            _ => {
                return Err(Malformed);
            }
        }
        Ok(())
    }
}

macro_rules! read_kvarray {
    ($p:ident, $($key:pat => $b:block)*) => {
        $p.read_kvarray(|$p, key| {
            match key {
                $($key => $b,)*
                _ => {$p.skip();}
            }
            Ok(())
        })?
    };
}

macro_rules! read_map {
    ($p:ident, $($key:pat => $b:block)*) => {
        $p.read_map(|$p, key| {
            match key {
                $($key => $b,)*
                _ => {$p.skip();}
            }
            Ok(())
        })?
    };
}

macro_rules! read_array {
    ($p:ident => $b:expr) => {
        $p.read_array(|$p| {
            while !$p.eof() {
                $b
            }
            Ok(())
        })?
    };
}

fn read_topology(p: &mut ParserImpl, geo: &mut Geo) -> Result<(), Error> {
    p.read_array(|p| match p.str()?.as_str() {
        "pointref" => p.read_array(|p| match p.str()?.as_str() {
            "indices" => p.read_array(|p| {
                while let Some(e) = p.next() {
                    geo.topology.push(e.as_integer().ok_or(Malformed)? as u32);
                }
                Ok(())
            }),
            _ => Err(Malformed),
        }),
        _ => Err(Malformed),
    })
}

fn read_point_attribute(p: &mut ParserImpl) -> Result<Attribute, Error> {
    let mut name = SmolStr::default();
    let mut storage = None;
    let mut size = 0;
    let mut storage_kind = StorageKind::Int32;

    //eprintln!("read_point_attribute metadata");

    p.begin_array()?;

    read_kvarray! {p,
        "name" => {
            name = p.str()?.into();
        }
    }

    //eprintln!("read_point_attribute data");
    read_kvarray! {p,
        "values" => {
            read_kvarray!(p,
                "size" => {
                    size = p.integer()? as usize;
                }
                "storage" => {
                    storage_kind = StorageKind::parse(&p.str()?)?;
                }
                "arrays" => {
                    storage = Some(AttributeStorage::new(storage_kind));
                    read_array! {p =>
                        read_array! {p =>
                            storage.as_mut().unwrap().read_element(p)?
                        }
                    }
                }
                "tuples" => {
                    storage = Some(AttributeStorage::new(storage_kind));
                    //eprintln!("read tuple data");
                    read_array! { p =>
                        read_array! { p =>
                            storage.as_mut().unwrap().read_element(p)?
                        }
                    }
                }
            );
        }
    }

    p.end_array()?;

    let Some(storage) = storage else {
        return Err(Error::Malformed);
    };
    Ok(Attribute { name, size, storage })
}

enum PrimType {
    Run,
}

enum RunType {
    BezierCurve,
}

fn read_bezier_basis(p: &mut ParserImpl) -> Result<BezierBasis, Error> {
    let mut ty = None;
    let mut order = 3;
    let mut knots = Vec::new();
    read_kvarray! {p,
        "type" => {
            ty = Some(p.str()?.to_string());
        }
        "order" => {
            order = p.integer()? as u32;
        }
        "knots" => {
            knots = p.read_fp32_array()?;
        }
    }
    Ok(BezierBasis { order, knots })
}

enum PrimitiveRun {
    BezierRun(BezierRun),
}

impl PrimitiveRun {
    fn read_uniform_fields(&mut self, p: &mut ParserImpl) -> Result<(), Error> {
        match self {
            PrimitiveRun::BezierRun(r) => read_map! {p,
                "vertex" => {
                    r.vertices = PrimVar::Uniform(p.read_int32_array()?);
                }
                "closed" => {
                    r.closed = PrimVar::Uniform(p.boolean()?);
                }
                "basis" => {
                    r.basis = PrimVar::Uniform(read_bezier_basis(p)?);
                }
            },
        }
        Ok(())
    }

    fn read_varying_fields(&mut self, fields: &[String], p: &mut ParserImpl) -> Result<(), Error> {
        match self {
            PrimitiveRun::BezierRun(r) => {
                let mut vertices = vec![];
                let mut basis = vec![];
                let mut closed = vec![];

                read_array! {p =>
                    // array of primitives
                    {
                        r.count += 1;
                        read_array!{p =>
                            // array of fields in the primitive
                            for f in fields {
                                match f.as_str() {
                                    "vertex" => {
                                        vertices.push(p.read_int32_array()?);
                                    }
                                    "closed" => {
                                        closed.push(p.boolean()?);
                                    }
                                    "basis" => {
                                        basis.push(read_bezier_basis(p)?);
                                    }
                                    _ => {
                                        p.skip();
                                    }
                                }
                            }
                        }
                    }
                }

                if !vertices.is_empty() {
                    r.vertices = PrimVar::Varying(vertices);
                }
                if !closed.is_empty() {
                    r.closed = PrimVar::Varying(closed);
                }
                if !basis.is_empty() {
                    r.basis = PrimVar::Varying(basis);
                }
            }
        }
        Ok(())
    }
}

fn read_primitives(p: &mut ParserImpl, geo: &mut Geo) -> Result<(), Error> {
    read_array! {p =>
        p.read_array(|p| {
            let mut prim_type = None;
            let mut varying_fields = Vec::new();
            let mut primitive_run = None;

            read_kvarray! {p,
                "type" => {
                    match p.str()?.as_str() {
                        "run" => prim_type = Some(PrimType::Run),
                        _ => {}
                    }
                }
                "runtype" => {
                    match p.str()?.as_str() {
                        "BezierCurve" => primitive_run = Some(PrimitiveRun::BezierRun(BezierRun::default())),
                        _ => {}
                    }
                }
                "varyingfields" => {
                    read_array!(p => varying_fields.push(p.str()?.to_string()));
                }
                "uniformfields" => {
                    let primitive_run = primitive_run.as_mut().ok_or(Malformed)?;
                    primitive_run.read_uniform_fields(p)?;
                }
            }

            {
                let primitive_run = primitive_run.as_mut().ok_or(Malformed)?;
                primitive_run.read_varying_fields(&varying_fields, p)?;
            }

            match primitive_run {
                Some(PrimitiveRun::BezierRun(r)) => {
                    geo.primitives.push(Primitive::BezierRun(r));
                }
                _ => {}
            }

            Ok(())
        })?
    }
    Ok(())
}

fn read_attributes(p: &mut ParserImpl, geo: &mut Geo) -> Result<(), Error> {
    read_kvarray! {p,
        "pointattributes" => {
            read_array!(p => {
                geo.point_attributes.push(read_point_attribute(p)?);
            })
        }
        "primitiveattributes" => {
            read_array!(p => {
                geo.primitive_attributes.push(read_point_attribute(p)?);
            })
        }
    }
    Ok(())
}

fn read_file(p: &mut ParserImpl) -> Result<Geo, Error> {
    let mut geo = Geo::default();
    read_kvarray! {p,
        "pointcount" => { geo.point_count = p.integer()? as usize}
        "vertexcount" => {geo.vertex_count = p.integer()? as usize}
        "primitivecount" =>{ geo.primitive_count = p.integer()? as usize}
        "topology" => {read_topology(p, &mut geo)?}
        "attributes" => {read_attributes(p, &mut geo)?}
        "primitives" => {read_primitives(p, &mut geo)?}
    }

    // Sanity checks for the position attribute.
    // TODO make this errors instead of panics
    assert!(
        geo.point_attributes.len() > 0,
        "the geometry should contain at least one point attribute"
    );
    let positions = &geo.point_attributes[0];
    assert_eq!(positions.name, "P", "the first point attribute should be the point position");
    assert_eq!(positions.size, 3, "the position attribute should have 3 components");
    assert!(positions.as_f32_slice().is_some(), "the position attribute should be fpreal32");
    let positions_fp32 = positions.as_f32_slice().unwrap();
    assert!(positions_fp32.len() % 3 == 0, "the number of positions should be a multiple of 3");
    assert_eq!(positions_fp32.len(), geo.point_count * 3);

    Ok(geo)
}

pub(crate) fn parse_json(str: &str) -> Result<Geo, Error> {
    let mut parser = ParserImpl::new(str);
    let geo = read_file(&mut parser)?;
    Ok(geo)
}

//! Types & constants for interfacing between shader and application code.
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::marker::PhantomData;
use graal::DeviceAddress;


include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

static vtx_main: VertexShader = VertexShader {
    name: "vtx_main",
    code: include_bytes!("../shaders/vtx_main.spv"),  
};
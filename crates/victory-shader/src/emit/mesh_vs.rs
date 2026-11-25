//! The vertex shader that draws what a mesh program run as compute wrote
//! ([`super::MeshLayout`]), reading `u0`-`u2` as `t0`-`t2` in
//! [`MESH_OUT_SPACE`]. Its outputs are the mesh program's output
//! signature, so the title's pixel shader (which links by semantic) stays
//! as it is.
//!
//! * Without per-primitive outputs it is drawn indexed (`u1` the index
//!   buffer): vertex `SV_VertexID` is vertex record `SV_VertexID`.
//! * With them, it is drawn unindexed, three vertices per primitive:
//!   vertex `SV_VertexID` is corner `SV_VertexID % 3` of primitive
//!   `SV_VertexID / 3`. It reads the corner's vertex index from `t1` and
//!   writes the primitive's record from `t2` as constant-interpolated
//!   outputs after the vertex outputs: the same value at every corner.

use super::bitcode::{Attr, Bin, Cast, Term, V};
use super::container::SigElement;
use super::shader::{MESH_OUT_SPACE, Shader};

/// Writes the four-dword record elements `elements` of the raw buffer
/// `t<register>` at byte `base` as outputs `first..`.
fn copy_record(s: &mut Shader, register: u8, base: V, elements: &[SigElement], first: usize) {
    let e = s.entry;
    let i32t = s.t.i32;
    let h = s.mesh_in_handle(register);
    for (k, el) in elements.iter().enumerate() {
        for col in 0..el.cols {
            let at = s.m.i32((k as u32 * 4 + u32::from(col)) * 4);
            let off = s.m.bin(e, Bin::Add, base, at);
            let x = load(s, h, off);
            let (suffix, x) = if el.comp == 9 {
                (".f32", x)
            } else {
                (".i32", s.m.cast(e, Cast::Bitcast, x, i32t))
            };
            let args = [s.m.i32((first + k) as u32), s.m.i32(0), s.m.i8(col), x];
            let void = s.t.void;
            s.op(
                e,
                &format!("storeOutput{suffix}"),
                Attr::NoUnwind,
                void,
                5,
                &args,
            );
        }
    }
}

/// The dword at byte `off` of raw buffer `h`, as a float.
fn load(s: &mut Shader, h: V, off: V) -> V {
    let e = s.entry;
    let (u, mask, al) = (s.m.undef(s.t.i32), s.m.i8(1), s.m.i32(4));
    let r = s.op(
        e,
        "rawBufferLoad.f32",
        Attr::ReadOnly,
        s.t.res_ret,
        139,
        &[h, off, u, mask, al],
    );
    s.m.extract(e, r, 0)
}

/// The vertex shader for a mesh program with these output and
/// per-primitive elements (in signature order, as the program was lowered
/// with) and record sizes in dwords; unindexed when `prims` is not empty.
pub(crate) fn vertex_shader(
    outputs: &[SigElement],
    prims: &[SigElement],
    vertex_dwords: u32,
    primitive_dwords: u32,
) -> Vec<u8> {
    let mut s = Shader::new(false);
    s.inputs = vec![SigElement {
        name: "SV_VertexID".into(),
        index: 0,
        kind: 1,
        comp: 5,
        interp: 0,
        row: Some(0),
        col: 0,
        cols: 1,
        usage: 1,
        half: false,
    }];
    let e = s.entry;
    let i32t = s.t.i32;
    let args = [s.m.i32(0), s.m.i32(0), s.m.i8(0), s.m.undef(i32t)];
    let id = s.op(e, "loadInput.i32", Attr::ReadNone, i32t, 4, &args);
    let vertex = if prims.is_empty() {
        id
    } else {
        // The corner's index: triangles are three consecutive dwords.
        let four = s.m.i32(4);
        let at = s.m.bin(e, Bin::Mul, id, four);
        let h = s.mesh_in_handle(1);
        let x = load(&mut s, h, at);
        s.m.cast(e, Cast::Bitcast, x, i32t)
    };
    let record = s.m.i32(vertex_dwords * 4);
    let base = s.m.bin(e, Bin::Mul, vertex, record);
    copy_record(&mut s, 0, base, outputs, 0);
    // Per-primitive outputs go in rows after the vertex outputs, constant.
    let first_row = outputs
        .iter()
        .filter_map(|o| o.row.map(|r| r + 1))
        .max()
        .unwrap_or(0);
    let low = prims.iter().filter_map(|p| p.row).min().unwrap_or(0);
    let mut placed: Vec<SigElement> = prims
        .iter()
        .cloned()
        .map(|mut p| {
            p.row = p.row.map(|r| first_row + r - low);
            p.interp = 1;
            p
        })
        .collect();
    if !prims.is_empty() {
        let three = s.m.i32(3);
        let primitive = s.m.bin(e, Bin::UDiv, id, three);
        let record = s.m.i32(primitive_dwords * 4);
        let base = s.m.bin(e, Bin::Mul, primitive, record);
        copy_record(&mut s, 2, base, prims, outputs.len());
    }
    let mut all: Vec<SigElement> = outputs.to_vec();
    all.append(&mut placed);
    s.outputs = all
        .into_iter()
        .map(|mut el| {
            el.usage = (1 << el.cols) - 1;
            el
        })
        .collect();
    s.m.terminate(e, Term::Ret);
    debug_assert_eq!(MESH_OUT_SPACE, super::MESH_OUT_SPACE);
    s.finish()
}

//! Converts indirect mesh-dispatch records into compute and draw commands
//! for adapters without native mesh shaders.
//!
//! The title's records are first clamped by `crate::indirect`. This pass
//! reads those bounded records and writes two command streams: one for the
//! compute producer and one for the generated-vertex draw. The output
//! records for each dispatch are placed consecutively in the data scratch
//! buffer, so t1 (indices) and t2 (per-primitive data) can stay bound at the
//! start of their regions for the whole draw pass.

use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;
use windows::core::s;

/// Maximum output bytes reserved for one indirect mesh execution batch.
pub(crate) const BYTES: u64 = 32 << 20;
/// D3D12's maximum dispatch group count in one dimension.
pub(crate) const MAX_GROUPS_PER_DISPATCH: u32 = 65_535;
/// Three root UAV addresses, then one dispatch, padded to 8-byte alignment.
pub(crate) const COMPUTE_STRIDE: u32 = 40;

pub(crate) const fn draw_stride(expanded: bool) -> u32 {
    if expanded { 24 } else { 32 }
}

const SOURCE: &str = r#"
#define COMPUTE_STRIDE 40
#define RS "RootConstants(num32BitConstants=16, b0), UAV(u0), UAV(u1), UAV(u2)"
cbuffer Plan : register(b0) {
    uint records; uint stride; uint counted; uint capacity;
    uint vertex_bytes; uint primitives; uint prim_bytes; uint expanded;
    uint2 vertices; uint2 indices; uint2 prims; uint draws; uint unused;
};
RWByteAddressBuffer args : register(u0);
RWByteAddressBuffer count : register(u1);
RWByteAddressBuffer commands : register(u2);
uint2 at(uint2 base, uint offset) {
    uint lo = base.x + offset;
    return uint2(lo, base.y + (lo < base.x ? 1u : 0u));
}
[RootSignature(RS)]
[numthreads(1, 1, 1)]
void main() {
    uint n = counted != 0 ? min(count.Load(0), records) : records;
    uint used = 0;
    for (uint i = 0; i < records; i++) {
        uint g = i < n ? args.Load(i * stride) : 0u;
        uint y = i < n ? args.Load(i * stride + 4) : 1u;
        uint z = i < n ? args.Load(i * stride + 8) : 1u;
        // The current shader lowering flattens only one-dimensional mesh
        // groups. Refuse other shapes rather than silently misindex outputs.
        if (y != 1u || z != 1u || g > capacity - used) {
            g = 0;
        }
        uint c = i * COMPUTE_STRIDE;
        commands.Store2(c, at(vertices, used * vertex_bytes));
        commands.Store2(c + 8, at(indices, used * primitives * 12));
        commands.Store2(c + 16, at(prims, used * prim_bytes));
        commands.Store3(c + 24, uint3(g, 1, 1));
        uint first = used * primitives * 3;
        uint d = draws + i * (expanded != 0 ? 24 : 32);
        commands.Store2(d, at(vertices, used * vertex_bytes));
        commands.Store4(d + 8, uint4(primitives * 3 * g, 1, first, 0));
        if (expanded == 0) {
            commands.Store(d + 24, 0);
        }
        used += g;
    }
}
"#;

pub(crate) struct Convert {
    pub(crate) root: ID3D12RootSignature,
    pub(crate) pso: ID3D12PipelineState,
}

// SAFETY: D3D12 objects are free-threaded.
unsafe impl Send for Convert {}
// SAFETY: as above.
unsafe impl Sync for Convert {}

/// The argument-to-command conversion pass, compiled once per host device.
pub(crate) fn convert() -> Option<&'static Convert> {
    static CONVERT: std::sync::OnceLock<Option<Convert>> = std::sync::OnceLock::new();
    CONVERT
        .get_or_init(|| {
            let device = &crate::host::host()?.device;
            match make(device) {
                Ok(convert) => Some(convert),
                Err(e) => {
                    tracing::warn!(error = %e, "no mesh fallback argument pass; indirect mesh dispatches dropped");
                    None
                }
            }
        })
        .as_ref()
}

fn make(device: &ID3D12Device) -> windows::core::Result<Convert> {
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // SAFETY: source and entry names live through the call; out pointers are ours.
    let compiled = unsafe {
        D3DCompile(
            SOURCE.as_ptr().cast(),
            SOURCE.len(),
            s!("mesh_indirect"),
            None,
            None,
            s!("main"),
            s!("cs_5_1"),
            0,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(e) = compiled {
        if let Some(errors) = errors {
            // SAFETY: the blob owns the returned bytes.
            let text = unsafe {
                std::slice::from_raw_parts(
                    errors.GetBufferPointer().cast::<u8>(),
                    errors.GetBufferSize(),
                )
            };
            tracing::warn!("mesh fallback argument pass: {}", String::from_utf8_lossy(text));
        }
        return Err(e);
    }
    let code = code.ok_or_else(windows::core::Error::empty)?;
    // SAFETY: the root signature is embedded in the compiled shader blob.
    unsafe {
        let bytes =
            std::slice::from_raw_parts(code.GetBufferPointer().cast::<u8>(), code.GetBufferSize());
        let root: ID3D12RootSignature = device.CreateRootSignature(0, bytes)?;
        let pso = device.CreateComputePipelineState(&D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: std::mem::transmute_copy(&root),
            CS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: bytes.as_ptr().cast(),
                BytecodeLength: bytes.len(),
            },
            ..Default::default()
        })?;
        Ok(Convert { root, pso })
    }
}

/// The command signatures for the compute producer and generated-vertex draw.
pub(crate) struct Signatures {
    pub(crate) compute: ID3D12CommandSignature,
    pub(crate) draw: ID3D12CommandSignature,
}

pub(crate) fn signatures(
    device: &ID3D12Device,
    root: &ID3D12RootSignature,
    outputs: [u32; 3],
    vertex: u32,
    expanded: bool,
) -> windows::core::Result<Signatures> {
    let uav = |index| D3D12_INDIRECT_ARGUMENT_DESC {
        Type: D3D12_INDIRECT_ARGUMENT_TYPE_UNORDERED_ACCESS_VIEW,
        Anonymous: D3D12_INDIRECT_ARGUMENT_DESC_0 {
            UnorderedAccessView: D3D12_INDIRECT_ARGUMENT_DESC_0_4 {
                RootParameterIndex: index,
            },
        },
    };
    let plain = |kind| D3D12_INDIRECT_ARGUMENT_DESC {
        Type: kind,
        ..Default::default()
    };
    let compute = [
        uav(outputs[0]),
        uav(outputs[1]),
        uav(outputs[2]),
        plain(D3D12_INDIRECT_ARGUMENT_TYPE_DISPATCH),
    ];
    let draw = [
        D3D12_INDIRECT_ARGUMENT_DESC {
            Type: D3D12_INDIRECT_ARGUMENT_TYPE_SHADER_RESOURCE_VIEW,
            Anonymous: D3D12_INDIRECT_ARGUMENT_DESC_0 {
                ShaderResourceView: D3D12_INDIRECT_ARGUMENT_DESC_0_3 {
                    RootParameterIndex: vertex,
                },
            },
        },
        plain(if expanded {
            D3D12_INDIRECT_ARGUMENT_TYPE_DRAW
        } else {
            D3D12_INDIRECT_ARGUMENT_TYPE_DRAW_INDEXED
        }),
    ];
    let make = |args: &[D3D12_INDIRECT_ARGUMENT_DESC], stride| {
        let desc = D3D12_COMMAND_SIGNATURE_DESC {
            ByteStride: stride,
            NumArgumentDescs: args.len() as u32,
            pArgumentDescs: args.as_ptr(),
            NodeMask: 0,
        };
        let mut signature: Option<ID3D12CommandSignature> = None;
        // SAFETY: the description points at live argument descriptors.
        unsafe { device.CreateCommandSignature(&desc, root, &mut signature)? };
        signature.ok_or_else(windows::core::Error::empty)
    };
    Ok(Signatures {
        compute: make(&compute, COMPUTE_STRIDE)?,
        draw: make(&draw, draw_stride(expanded))?,
    })
}

/// Buffer regions for a batch of indirect records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Regions {
    pub(crate) capacity: u32,
    pub(crate) indices: u64,
    pub(crate) prims: u64,
    pub(crate) index_bytes: u64,
    pub(crate) bytes: u64,
}

pub(crate) fn regions(layout: &victory_shader::emit::MeshLayout) -> Regions {
    let per_group: u64 = layout.bytes(1).iter().sum();
    let room = BYTES - 1024;
    let capacity = (room / per_group.max(1)).clamp(1, u64::from(u32::MAX)) as u32;
    let [v, i, p] = layout.bytes(capacity);
    let aligned = |n: u64| n.next_multiple_of(256);
    Regions {
        capacity,
        indices: aligned(v),
        prims: aligned(v) + aligned(i),
        index_bytes: i,
        bytes: aligned(v) + aligned(i) + p.max(16),
    }
}

/// The converter pass's 16 root constants.
#[allow(clippy::too_many_arguments)]
pub(crate) fn plan(
    records: u32,
    stride: u32,
    layout: &victory_shader::emit::MeshLayout,
    regions: &Regions,
    expanded: bool,
    data: u64,
    draws: u32,
) -> [u32; 16] {
    let split = |a: u64| [a as u32, (a >> 32) as u32];
    let [v, _, p] = layout.bytes(1);
    let [vl, vh] = split(data);
    let [il, ih] = split(data + regions.indices);
    let [pl, ph] = split(data + regions.prims);
    [
        records,
        stride,
        0, // The upstream clamp has already zeroed records beyond the count.
        regions.capacity,
        v as u32,
        layout.max_primitives,
        p as u32,
        u32::from(expanded),
        vl,
        vh,
        il,
        ih,
        pl,
        ph,
        draws,
        0,
    ]
}

/// Records the conversion pass. The caller keeps all buffers alive and
/// transitions them into UAV state before calling.
pub(crate) fn record(
    list: &ID3D12GraphicsCommandList,
    convert: &Convert,
    plan: &[u32; 16],
    args: u64,
    commands: u64,
) {
    // SAFETY: open command list; all root resources are retained by the list.
    unsafe {
        list.SetComputeRootSignature(&convert.root);
        list.SetPipelineState(&convert.pso);
        list.SetComputeRoot32BitConstants(0, 16, plan.as_ptr().cast(), 0);
        list.SetComputeRootUnorderedAccessView(1, args);
        list.SetComputeRootUnorderedAccessView(2, args);
        list.SetComputeRootUnorderedAccessView(3, commands);
        list.Dispatch(1, 1, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_fit_a_bounded_output_batch() {
        let layout = victory_shader::emit::MeshLayout {
            threads: 32,
            max_vertices: 64,
            max_primitives: 32,
            vertex_dwords: 16,
            primitive_dwords: 0,
        };
        let regions = regions(&layout);
        assert!(regions.capacity > 0);
        assert!(regions.indices.is_multiple_of(256));
        assert!(regions.prims.is_multiple_of(256));
        assert!(regions.bytes <= BYTES);
    }

    #[test]
    fn converter_hlsl_compiles_as_shader_model_5_1_compute() {
        let mut code: Option<ID3DBlob> = None;
        let mut errors: Option<ID3DBlob> = None;
        // SAFETY: the source and entry-point strings live through compilation.
        let result = unsafe {
            D3DCompile(
                SOURCE.as_ptr().cast(),
                SOURCE.len(),
                s!("mesh_indirect_test"),
                None,
                None,
                s!("main"),
                s!("cs_5_1"),
                0,
                0,
                &mut code,
                Some(&mut errors),
            )
        };
        if let Err(error) = result {
            let detail = errors.map(|blob| {
                // SAFETY: the error blob owns the bytes for this read.
                let bytes = unsafe {
                    std::slice::from_raw_parts(
                        blob.GetBufferPointer().cast::<u8>(),
                        blob.GetBufferSize(),
                    )
                };
                String::from_utf8_lossy(bytes).into_owned()
            });
            panic!("mesh indirect HLSL failed to compile: {error}; {detail:?}");
        }
        assert!(code.is_some(), "D3DCompile returned no bytecode");
    }

    #[test]
    fn command_strides_match_root_and_draw_arguments() {
        assert_eq!(COMPUTE_STRIDE, 40);
        assert_eq!(draw_stride(false), 32);
        assert_eq!(draw_stride(true), 24);
    }
}

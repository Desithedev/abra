use crate::gfx::Gfx;
use std::cell::RefCell;
use windows::core::{s, PCSTR};
use windows::UI::Composition::CompositionDrawingSurface;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R32G32_FLOAT, DXGI_SAMPLE_DESC,
};
use windows_core::Result;

const ROWS: usize = 72;
const VERTS: usize = (ROWS + 1) * 2;
const MAX_RT_DIM: f32 = 960.0;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CParams {
    opacity: f32,
    _pad: [f32; 3],
}

const HLSL: &[u8] = br#"
Texture2D tex0 : register(t0);
SamplerState smp0 : register(s0);
cbuffer Params : register(b0) {
    float opacity;
    float3 _pad;
};
struct VSIn {
    float2 pos : POSITION;
    float2 uv  : TEXCOORD0;
};
struct PSIn {
    float4 pos : SV_POSITION;
    float2 uv  : TEXCOORD0;
};
PSIn vs_main(VSIn i) {
    PSIn o;
    o.pos = float4(i.pos, 0.0, 1.0);
    o.uv = i.uv;
    return o;
}
float4 ps_main(PSIn i) : SV_TARGET {
    float4 c = tex0.Sample(smp0, i.uv);
    float fw = max(fwidth(i.uv.x), 0.0004);
    float aa_x = smoothstep(0.0, fw * 1.75, i.uv.x) * smoothstep(1.0, 1.0 - fw * 1.75, i.uv.x);
    float fwy = max(fwidth(i.uv.y), 0.0004);
    float aa_y = smoothstep(0.0, fwy * 1.25, i.uv.y) * smoothstep(1.0, 1.0 - fwy * 1.25, i.uv.y);
    return c * (aa_x * aa_y * opacity);
}
"#;

struct CachedTarget {
    w: u32,
    h: u32,
    rt_tex: ID3D11Texture2D,
    rtv: ID3D11RenderTargetView,
    surface: CompositionDrawingSurface,
}

pub struct GeniePipe {
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    layout: ID3D11InputLayout,
    vb: ID3D11Buffer,
    cb: ID3D11Buffer,
    sampler: ID3D11SamplerState,
    raster: ID3D11RasterizerState,
    cached_rt: RefCell<Option<CachedTarget>>,
}

pub struct GenieTex {
    srv: ID3D11ShaderResourceView,
    rt_tex: ID3D11Texture2D,
    rtv: ID3D11RenderTargetView,
    pub surface: CompositionDrawingSurface,
    pub canvas: (i32, i32, i32, i32),
    rt_size: (u32, u32),
}

fn compile_shader(entry: PCSTR, target: PCSTR) -> Result<ID3DBlob> {
    unsafe {
        let mut blob: Option<ID3DBlob> = None;
        D3DCompile(
            HLSL.as_ptr() as *const _,
            HLSL.len(),
            None,
            None,
            None,
            entry,
            target,
            0,
            0,
            &mut blob,
            None,
        )?;
        blob.ok_or_else(windows_core::Error::empty)
    }
}

impl GeniePipe {
    pub fn new(gfx: &Gfx) -> Result<GeniePipe> {
        let dev = gfx.device();
        unsafe {
            let vs_blob = compile_shader(s!("vs_main"), s!("vs_4_0"))?;
            let ps_blob = compile_shader(s!("ps_main"), s!("ps_4_0"))?;
            let vs_slice = std::slice::from_raw_parts(
                vs_blob.GetBufferPointer() as *const u8,
                vs_blob.GetBufferSize(),
            );
            let ps_slice = std::slice::from_raw_parts(
                ps_blob.GetBufferPointer() as *const u8,
                ps_blob.GetBufferSize(),
            );

            let mut vs: Option<ID3D11VertexShader> = None;
            dev.CreateVertexShader(vs_slice, None, Some(&mut vs))?;
            let vs = vs.ok_or_else(windows_core::Error::empty)?;

            let mut ps: Option<ID3D11PixelShader> = None;
            dev.CreatePixelShader(ps_slice, None, Some(&mut ps))?;
            let ps = ps.ok_or_else(windows_core::Error::empty)?;

            let elems = [
                D3D11_INPUT_ELEMENT_DESC {
                    SemanticName: s!("POSITION"),
                    SemanticIndex: 0,
                    Format: DXGI_FORMAT_R32G32_FLOAT,
                    InputSlot: 0,
                    AlignedByteOffset: 0,
                    InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                    InstanceDataStepRate: 0,
                },
                D3D11_INPUT_ELEMENT_DESC {
                    SemanticName: s!("TEXCOORD"),
                    SemanticIndex: 0,
                    Format: DXGI_FORMAT_R32G32_FLOAT,
                    InputSlot: 0,
                    AlignedByteOffset: 8,
                    InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                    InstanceDataStepRate: 0,
                },
            ];
            let mut layout: Option<ID3D11InputLayout> = None;
            dev.CreateInputLayout(&elems, vs_slice, Some(&mut layout))?;
            let layout = layout.ok_or_else(windows_core::Error::empty)?;

            let vb_desc = D3D11_BUFFER_DESC {
                ByteWidth: (VERTS * std::mem::size_of::<Vertex>()) as u32,
                Usage: D3D11_USAGE_DYNAMIC,
                BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
                CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                ..Default::default()
            };
            let mut vb: Option<ID3D11Buffer> = None;
            dev.CreateBuffer(&vb_desc, None, Some(&mut vb))?;
            let vb = vb.ok_or_else(windows_core::Error::empty)?;

            let cb_desc = D3D11_BUFFER_DESC {
                ByteWidth: std::mem::size_of::<CParams>() as u32,
                Usage: D3D11_USAGE_DYNAMIC,
                BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                ..Default::default()
            };
            let mut cb: Option<ID3D11Buffer> = None;
            dev.CreateBuffer(&cb_desc, None, Some(&mut cb))?;
            let cb = cb.ok_or_else(windows_core::Error::empty)?;

            let smp_desc = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                ComparisonFunc: D3D11_COMPARISON_NEVER,
                MaxLOD: f32::MAX,
                ..Default::default()
            };
            let mut sampler: Option<ID3D11SamplerState> = None;
            dev.CreateSamplerState(&smp_desc, Some(&mut sampler))?;
            let sampler = sampler.ok_or_else(windows_core::Error::empty)?;

            let rs_desc = D3D11_RASTERIZER_DESC {
                FillMode: D3D11_FILL_SOLID,
                CullMode: D3D11_CULL_NONE,
                ScissorEnable: false.into(),
                MultisampleEnable: true.into(),
                AntialiasedLineEnable: true.into(),
                ..Default::default()
            };
            let mut raster: Option<ID3D11RasterizerState> = None;
            dev.CreateRasterizerState(&rs_desc, Some(&mut raster))?;
            let raster = raster.ok_or_else(windows_core::Error::empty)?;

            Ok(GeniePipe {
                vs,
                ps,
                layout,
                vb,
                cb,
                sampler,
                raster,
                cached_rt: RefCell::new(None),
            })
        }
    }

    pub fn prepare(
        &self,
        gfx: &Gfx,
        w: u32,
        h: u32,
        bgra: &[u8],
        win: (i32, i32, i32, i32),
        icon: (i32, i32, i32, i32),
    ) -> Result<GenieTex> {
        let dev = gfx.device();
        let pad = 12;
        let x0 = win.0.min(icon.0) - pad;
        let y0 = win.1.min(icon.1) - pad;
        let x1 = (win.0 + win.2).max(icon.0 + icon.2) + pad;
        let y1 = (win.1 + win.3).max(icon.1 + icon.3) + pad;
        let cw = (x1 - x0).clamp(64, 4096);
        let ch = (y1 - y0).clamp(64, 4096);

        let scale = (MAX_RT_DIM / (cw.max(ch) as f32)).min(1.0);
        let rtw = ((cw as f32 * scale).round() as u32).clamp(64, 2048);
        let rth = ((ch as f32 * scale).round() as u32).clamp(64, 2048);

        unsafe {
            let src_desc = D3D11_TEXTURE2D_DESC {
                Width: w,
                Height: h,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_IMMUTABLE,
                BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                ..Default::default()
            };
            let init = D3D11_SUBRESOURCE_DATA {
                pSysMem: bgra.as_ptr() as *const _,
                SysMemPitch: w * 4,
                SysMemSlicePitch: 0,
            };
            let mut src_tex: Option<ID3D11Texture2D> = None;
            dev.CreateTexture2D(&src_desc, Some(&init), Some(&mut src_tex))?;
            let src_tex = src_tex.ok_or_else(windows_core::Error::empty)?;

            let mut srv: Option<ID3D11ShaderResourceView> = None;
            dev.CreateShaderResourceView(&src_tex, None, Some(&mut srv))?;
            let srv = srv.ok_or_else(windows_core::Error::empty)?;

            let reuse = self
                .cached_rt
                .borrow()
                .as_ref()
                .filter(|c| c.w == rtw && c.h == rth)
                .map(|c| (c.rt_tex.clone(), c.rtv.clone(), c.surface.clone()));

            let (rt_tex, rtv, surface) = if let Some(hit) = reuse {
                hit
            } else {
                let rt_desc = D3D11_TEXTURE2D_DESC {
                    Width: rtw,
                    Height: rth,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                    ..Default::default()
                };
                let mut rt_tex: Option<ID3D11Texture2D> = None;
                dev.CreateTexture2D(&rt_desc, None, Some(&mut rt_tex))?;
                let rt_tex = rt_tex.ok_or_else(windows_core::Error::empty)?;

                let mut rtv: Option<ID3D11RenderTargetView> = None;
                dev.CreateRenderTargetView(&rt_tex, None, Some(&mut rtv))?;
                let rtv = rtv.ok_or_else(windows_core::Error::empty)?;

                let surface = gfx.new_surface(rtw, rth)?;
                *self.cached_rt.borrow_mut() = Some(CachedTarget {
                    w: rtw,
                    h: rth,
                    rt_tex: rt_tex.clone(),
                    rtv: rtv.clone(),
                    surface: surface.clone(),
                });
                (rt_tex, rtv, surface)
            };

            Ok(GenieTex {
                srv,
                rt_tex,
                rtv,
                surface,
                canvas: (x0, y0, cw, ch),
                rt_size: (rtw, rth),
            })
        }
    }

    pub fn draw_frame(
        &self,
        gfx: &Gfx,
        tex: &GenieTex,
        win: (i32, i32, i32, i32),
        icon: (i32, i32, i32, i32),
        p: f32,
        opacity: f32,
    ) -> Result<()> {
        let (cx0, cy0, cw_i, ch_i) = tex.canvas;
        let (cw, ch) = (cw_i as f32, ch_i as f32);
        let (wx, wy, ww, wh) = (
            (win.0 - cx0) as f32,
            (win.1 - cy0) as f32,
            win.2.max(1) as f32,
            win.3.max(1) as f32,
        );
        let (ix, iy, iw, ih) = (
            (icon.0 - cx0) as f32,
            (icon.1 - cy0) as f32,
            icon.2.max(1) as f32,
            icon.3.max(1) as f32,
        );

        let dock_y = iy + ih * 0.5;
        let p = p.clamp(0.0, 1.0);

        // Continuous overlapping phases so velocity never drops to zero mid-animation
        let u_p = (p / 0.44).clamp(0.0, 1.0);
        let pinch = 1.0 - (1.0 - u_p).powi(3);

        let u_b = (p / 0.36).clamp(0.0, 1.0);
        let bot_st = u_b * u_b * (3.0 - 2.0 * u_b);
        let bot_y = (wy + wh) + (dock_y - (wy + wh)) * bot_st;

        let u_t = ((p - 0.10) / 0.90).clamp(0.0, 1.0);
        let top_st = u_t * u_t * (3.0 - 2.0 * u_t);
        let top_y = wy + (dock_y - wy) * top_st;

        let span_y = (dock_y - wy).abs().max(1.0);
        let to_ndc = |px: f32, py: f32| -> [f32; 2] {
            [(px / cw) * 2.0 - 1.0, 1.0 - (py / ch) * 2.0]
        };

        let mut verts = [Vertex::default(); VERTS];
        for i in 0..=ROWS {
            let u = i as f32 / ROWS as f32;
            let curr_y = top_y + u * (bot_y - top_y);
            let v = ((curr_y - wy).abs() / span_y).clamp(0.0, 1.0);
            let s = 0.5 - 0.5 * (std::f32::consts::PI * v).cos();
            let left = wx + (ix - wx) * s * pinch;
            let right = (wx + ww) + ((ix + iw) - (wx + ww)) * s * pinch;
            verts[i * 2] = Vertex {
                pos: to_ndc(left, curr_y),
                uv: [0.0, u],
            };
            verts[i * 2 + 1] = Vertex {
                pos: to_ndc(right, curr_y),
                uv: [1.0, u],
            };
        }

        let (rtw, rth) = tex.rt_size;
        let ctx = gfx.ctx();
        unsafe {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            ctx.Map(&self.vb, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))?;
            std::ptr::copy_nonoverlapping(verts.as_ptr(), mapped.pData as *mut Vertex, VERTS);
            ctx.Unmap(&self.vb, 0);

            let mut cb_map = D3D11_MAPPED_SUBRESOURCE::default();
            ctx.Map(&self.cb, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut cb_map))?;
            *(cb_map.pData as *mut CParams) = CParams {
                opacity,
                _pad: [0.0; 3],
            };
            ctx.Unmap(&self.cb, 0);

            ctx.ClearRenderTargetView(&tex.rtv, &[0.0, 0.0, 0.0, 0.0]);
            let rtvs = [Some(tex.rtv.clone())];
            ctx.OMSetRenderTargets(Some(&rtvs), None);
            let vp = [D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: rtw as f32,
                Height: rth as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }];
            ctx.RSSetViewports(Some(&vp));
            ctx.RSSetState(&self.raster);
            ctx.IASetInputLayout(&self.layout);
            let stride = std::mem::size_of::<Vertex>() as u32;
            let offset = 0u32;
            let vbs = [Some(self.vb.clone())];
            ctx.IASetVertexBuffers(0, 1, Some(vbs.as_ptr()), Some(&stride), Some(&offset));
            ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            ctx.VSSetShader(&self.vs, None);
            ctx.PSSetShader(&self.ps, None);
            let cbs = [Some(self.cb.clone())];
            ctx.PSSetConstantBuffers(0, Some(&cbs));
            let srvs = [Some(tex.srv.clone())];
            ctx.PSSetShaderResources(0, Some(&srvs));
            let smps = [Some(self.sampler.clone())];
            ctx.PSSetSamplers(0, Some(&smps));
            ctx.Draw(VERTS as u32, 0);
        }

        gfx.copy_into(&tex.surface, &tex.rt_tex, rtw, rth)
    }
}

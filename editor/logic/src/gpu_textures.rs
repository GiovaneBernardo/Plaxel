//! GPU-only inspection of the previous completed frame; no synchronous readback.
use egui_wgpu::wgpu;
use engine::renderer::{TextureHandle, wgpu_backend::WgpuBackend};

type Entry = (Option<TextureHandle>, String, wgpu::Texture);

#[derive(Default)]
pub struct GpuTextureExplorer {
    entries: Vec<Entry>,
    selected: Option<Option<TextureHandle>>,
    filter: String,
    mip: u32,
    layer: u32,
    channel: usize,
    exposure: f32,
    zoom: f32,
    preview: Option<Preview>,
}

struct Preview {
    source: wgpu::Texture,
    settings: (u32, u32, usize, u32),
    target: wgpu::Texture,
    pipeline: wgpu::RenderPipeline,
    binding: wgpu::BindGroup,
    id: egui::TextureId,
}

impl GpuTextureExplorer {
    pub fn reset_renderer(&mut self) {
        self.preview = None;
    }

    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool) {
        egui::Window::new("GPU Texture Explorer")
            .open(open)
            .default_size([900.0, 650.0])
            .show(ctx, |ui| {
                ui.label("Live backend textures • preview from the previous completed frame");
                ui.horizontal(|ui| {
                    ui.label(format!("{} textures", self.entries.len()));
                    ui.text_edit_singleline(&mut self.filter);
                });
                let filter = self.filter.to_lowercase();
                egui::ScrollArea::vertical()
                    .id_salt("gpu_texture_list")
                    .max_height(170.0)
                    .show(ui, |ui| {
                        for (handle, label, texture) in &self.entries {
                            if !label.to_lowercase().contains(&filter) {
                                continue;
                            }
                            let text = format!(
                                "{}  [{}]  {} × {} × {}  {:?}",
                                label,
                                handle.map_or_else(|| "depth".into(), |h| h.0.to_string()),
                                texture.width(),
                                texture.height(),
                                texture.depth_or_array_layers(),
                                texture.format()
                            );
                            if ui
                                .selectable_label(self.selected == Some(*handle), text)
                                .clicked()
                            {
                                self.selected = Some(*handle);
                                self.mip = 0;
                                self.layer = 0;
                            }
                        }
                    });
                ui.separator();
                let Some((_, label, texture)) =
                    self.entries.iter().find(|e| Some(e.0) == self.selected)
                else {
                    ui.label("Select a texture to inspect.");
                    return;
                };
                ui.strong(label);
                ui.label(format!(
                    "{:?} • {:?} • {} mips • {} samples",
                    texture.dimension(),
                    texture.usage(),
                    texture.mip_level_count(),
                    texture.sample_count()
                ));
                if let Some(reason) = unsupported(texture) {
                    ui.label(reason);
                    return;
                }
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Slider::new(&mut self.mip, 0..=texture.mip_level_count() - 1)
                            .text("Mip"),
                    );
                    let layers = if texture.dimension() == wgpu::TextureDimension::D3 {
                        (texture.depth_or_array_layers() >> self.mip).max(1)
                    } else {
                        texture.depth_or_array_layers()
                    };
                    self.layer = self.layer.min(layers - 1);
                    ui.add(
                        egui::Slider::new(&mut self.layer, 0..=layers - 1).text("Layer / slice"),
                    );
                });
                ui.horizontal(|ui| {
                    for (i, name) in ["RGB", "R", "G", "B", "A"].iter().enumerate() {
                        ui.selectable_value(&mut self.channel, i, *name);
                    }
                    ui.add(egui::Slider::new(&mut self.exposure, -16.0..=16.0).text("Exposure"));
                    if ui.button("Reset").clicked() {
                        self.exposure = 0.0;
                        self.channel = 0;
                        self.zoom = 0.0;
                    }
                });
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.zoom, 0.0, "Fit");
                    ui.selectable_value(&mut self.zoom, 1.0, "1:1");
                    ui.selectable_value(&mut self.zoom, 2.0, "2×");
                    ui.selectable_value(&mut self.zoom, 4.0, "4×");
                });
                if let Some(preview) = &self.preview {
                    // Do not show stale content while the new selection is being prepared.
                    if preview.source != *texture || preview.settings != self.settings() {
                        return;
                    }
                    let size = egui::vec2(
                        preview.target.width() as f32,
                        preview.target.height() as f32,
                    );
                    let scale = if self.zoom == 0.0 {
                        (ui.available_width() / size.x).min(ui.available_height().max(1.0) / size.y)
                    } else {
                        self.zoom
                    };
                    egui::ScrollArea::both()
                        .id_salt("gpu_texture_preview")
                        .show(ui, |ui| {
                            ui.image((preview.id, size * scale));
                        });
                }
            });
    }

    fn settings(&self) -> (u32, u32, usize, u32) {
        (self.mip, self.layer, self.channel, self.exposure.to_bits())
    }

    pub fn prepare(
        &mut self,
        backend: &WgpuBackend,
        renderer: &mut egui_wgpu::Renderer,
        open: bool,
    ) {
        if !open {
            if let Some(old) = self.preview.take() {
                renderer.free_texture(&old.id);
            }
            self.entries.clear();
            return;
        }
        self.entries = backend.debug_textures();
        let source = self
            .entries
            .iter()
            .find(|e| Some(e.0) == self.selected)
            .map(|e| e.2.clone());
        let Some(source) = source.filter(|t| unsupported(t).is_none()) else {
            if let Some(old) = self.preview.take() {
                renderer.free_texture(&old.id);
            }
            return;
        };
        self.mip = self.mip.min(source.mip_level_count() - 1);
        let layers = if source.dimension() == wgpu::TextureDimension::D3 {
            (source.depth_or_array_layers() >> self.mip).max(1)
        } else {
            source.depth_or_array_layers()
        };
        self.layer = self.layer.min(layers - 1);
        let settings = self.settings();
        if self
            .preview
            .as_ref()
            .is_none_or(|p| p.source != source || p.settings != settings)
        {
            let mut preview = create_preview(backend.device(), renderer, source, settings);
            if let Some(old) = self.preview.take() {
                // The UI may already reference this ID when a resize replaces the source.
                renderer.free_texture(&preview.id);
                preview.id = old.id;
                renderer.update_egui_texture_from_wgpu_texture(
                    backend.device(),
                    &preview.target.create_view(&Default::default()),
                    wgpu::FilterMode::Nearest,
                    preview.id,
                );
            }
            self.preview = Some(preview);
        }
        let p = self.preview.as_ref().unwrap();
        let view = p.target.create_view(&Default::default());
        let mut encoder =
            backend
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("GPU texture preview"),
                });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("GPU texture preview"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&p.pipeline);
            pass.set_bind_group(0, &p.binding, &[]);
            pass.draw(0..3, 0..1);
        }
        backend.queue().submit([encoder.finish()]);
    }
}

fn unsupported(texture: &wgpu::Texture) -> Option<&'static str> {
    if !texture
        .usage()
        .contains(wgpu::TextureUsages::TEXTURE_BINDING)
    {
        return Some("Preview unavailable: this allocation has no TEXTURE_BINDING usage.");
    }
    if texture.sample_count() != 1 {
        return Some("Preview unavailable for multisampled textures.");
    }
    if texture.dimension() == wgpu::TextureDimension::D1 {
        return Some("Preview unavailable for 1D textures.");
    }
    if texture
        .format()
        .sample_type(Some(wgpu::TextureAspect::All), None)
        .is_none()
        && !texture.format().is_depth_stencil_format()
    {
        return Some("Preview unavailable for this format.");
    }
    if texture.format() == wgpu::TextureFormat::Stencil8 {
        return Some("Preview unavailable for stencil-only textures.");
    }
    None
}

fn create_preview(
    device: &wgpu::Device,
    renderer: &mut egui_wgpu::Renderer,
    source: wgpu::Texture,
    settings: (u32, u32, usize, u32),
) -> Preview {
    let (mip, layer, channel, exposure) = settings;
    let depth = source.format().is_depth_stencil_format();
    let volume = source.dimension() == wgpu::TextureDimension::D3;
    let sample_type = if depth {
        wgpu::TextureSampleType::Depth
    } else {
        match source
            .format()
            .sample_type(Some(wgpu::TextureAspect::All), None)
            .unwrap()
        {
            wgpu::TextureSampleType::Float { .. } => {
                wgpu::TextureSampleType::Float { filterable: false }
            }
            other => other,
        }
    };
    let scalar = match sample_type {
        wgpu::TextureSampleType::Uint => "u32",
        wgpu::TextureSampleType::Sint => "i32",
        _ => "f32",
    };
    let declaration = if depth {
        "texture_depth_2d".into()
    } else {
        format!("texture_{}<{scalar}>", if volume { "3d" } else { "2d" })
    };
    let coord = if volume {
        format!("vec3<i32>(vec2<i32>(p.xy), {layer})")
    } else {
        "vec2<i32>(p.xy)".into()
    };
    let load = if depth {
        format!("vec4<f32>(vec3<f32>(textureLoad(src, {coord}, 0)), 1.0)")
    } else {
        format!("vec4<f32>(textureLoad(src, {coord}, 0))")
    };
    let color = if channel == 0 {
        "c.rgb".into()
    } else {
        format!("vec3<f32>(c[{}])", channel - 1)
    };
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("GPU texture preview shader"),
        source: wgpu::ShaderSource::Wgsl(format!(r#"
@group(0) @binding(0) var src: {declaration};
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{
    let p = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(p[i], 0.0, 1.0);
}}
@fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {{
    let c = {load};
    let rgb = clamp({color} * {gain:?}, vec3(0.0), vec3(1.0));
    // The egui native texture path expects gamma-encoded color.
    let srgb = select(1.055 * pow(rgb, vec3(1.0 / 2.4)) - 0.055, 12.92 * rgb, rgb <= vec3(0.0031308));
    return vec4(srgb, 1.0);
}}
"#, gain = 2.0f32.powf(f32::from_bits(exposure))).into()),
    });
    let dimension = if volume {
        wgpu::TextureViewDimension::D3
    } else {
        wgpu::TextureViewDimension::D2
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("GPU texture preview layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: dimension,
                multisampled: false,
            },
            count: None,
        }],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("GPU texture preview pipeline layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("GPU texture preview"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let source_view = source.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(dimension),
        aspect: if depth {
            wgpu::TextureAspect::DepthOnly
        } else {
            wgpu::TextureAspect::All
        },
        base_mip_level: mip,
        mip_level_count: Some(1),
        base_array_layer: if volume { 0 } else { layer },
        array_layer_count: if volume { None } else { Some(1) },
        ..Default::default()
    });
    let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("GPU texture preview binding"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&source_view),
        }],
    });
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("GPU texture explorer preview"),
        size: wgpu::Extent3d {
            width: (source.width() >> mip).max(1),
            height: (source.height() >> mip).max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let id = renderer.register_native_texture(
        device,
        &target.create_view(&Default::default()),
        wgpu::FilterMode::Nearest,
    );
    Preview {
        source,
        settings,
        target,
        pipeline,
        binding,
        id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_validates_color_depth_integer_array_and_volume() {
        pollster::block_on(async {
            let instance = wgpu::Instance::default();
            let adapter = instance
                .request_adapter(&Default::default())
                .await
                .expect("GPU adapter for preview validation");
            let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
            let mut renderer = egui_wgpu::Renderer::new(
                &device,
                wgpu::TextureFormat::Rgba8Unorm,
                Default::default(),
            );
            for (format, dimension, layers) in [
                (
                    wgpu::TextureFormat::Rgba16Float,
                    wgpu::TextureDimension::D2,
                    1,
                ),
                (wgpu::TextureFormat::R32Float, wgpu::TextureDimension::D2, 1),
                (wgpu::TextureFormat::R16Float, wgpu::TextureDimension::D3, 4),
                (
                    wgpu::TextureFormat::Rgba8UnormSrgb,
                    wgpu::TextureDimension::D2,
                    6,
                ),
                (
                    wgpu::TextureFormat::Rgba8Uint,
                    wgpu::TextureDimension::D2,
                    1,
                ),
                (
                    wgpu::TextureFormat::Rgba8Sint,
                    wgpu::TextureDimension::D2,
                    1,
                ),
                (
                    wgpu::TextureFormat::Depth32Float,
                    wgpu::TextureDimension::D2,
                    2,
                ),
                (
                    wgpu::TextureFormat::Depth24PlusStencil8,
                    wgpu::TextureDimension::D2,
                    1,
                ),
                (
                    wgpu::TextureFormat::Rgba8Unorm,
                    wgpu::TextureDimension::D3,
                    4,
                ),
            ] {
                let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
                let source = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("preview validation source"),
                    size: wgpu::Extent3d {
                        width: 8,
                        height: 8,
                        depth_or_array_layers: layers,
                    },
                    mip_level_count: 2,
                    sample_count: 1,
                    dimension,
                    format,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                let layer = if dimension == wgpu::TextureDimension::D3 {
                    layers / 2 - 1
                } else {
                    layers - 1
                };
                assert!(unsupported(&source).is_none());
                for channel in 0..5 {
                    let p = create_preview(
                        &device,
                        &mut renderer,
                        source.clone(),
                        (1, layer, channel, (-2.0f32).to_bits()),
                    );
                    let view = p.target.create_view(&Default::default());
                    let mut encoder = device.create_command_encoder(&Default::default());
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            ..Default::default()
                        });
                        pass.set_pipeline(&p.pipeline);
                        pass.set_bind_group(0, &p.binding, &[]);
                        pass.draw(0..3, 0..1);
                    }
                    queue.submit([encoder.finish()]);
                    renderer.free_texture(&p.id);
                }
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                let error = scope.pop().await;
                assert!(
                    error.is_none(),
                    "Preview validation failed for {format:?} {dimension:?}: {error:?}"
                );
            }
        });
    }
}

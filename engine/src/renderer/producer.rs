use std::{any::Any, collections::HashMap, mem::size_of, ops::Range};

use crate::assets::manager::Handle;
use crate::{
    model::{MeshAsset, TransformInstance},
    renderer::{
        BindGroupHandle, BufferDescriptor, BufferHandle, BufferUsages, GraphPassId, MaterialPassId,
        PipelineOverride, RenderContext, RenderPhaseId, RenderProducerId, RenderResources,
        RenderViewId, RendererAPI,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RenderViewKind {
    Main,
    ShadowCascade { cascade: u32 },
    Editor,
    Custom(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderViewSelector {
    Main,
    ShadowCascades,
    Editor,
    Custom(u64),
    All,
}

impl RenderViewSelector {
    pub fn matches(self, kind: RenderViewKind) -> bool {
        match (self, kind) {
            (Self::All, _) | (Self::Main, RenderViewKind::Main) => true,
            (Self::ShadowCascades, RenderViewKind::ShadowCascade { .. }) => true,
            (Self::Editor, RenderViewKind::Editor) => true,
            (Self::Custom(expected), RenderViewKind::Custom(actual)) => expected == actual,
            _ => false,
        }
    }
}

/// A camera/view participating in a graph pass. Custom producers can use the optional
/// bind group directly, while standard passes may bind it before producer recording.
#[derive(Debug, Clone)]
pub struct RenderView {
    pub id: RenderViewId,
    pub kind: RenderViewKind,
    pub view_bind_group: Option<BindGroupHandle>,
}

#[derive(Default)]
pub struct RenderViewRegistry {
    by_pass: HashMap<GraphPassId, Vec<RenderView>>,
}

impl RenderViewRegistry {
    pub fn set_views(&mut self, pass: GraphPassId, views: Vec<RenderView>) {
        self.by_pass.insert(pass, views);
    }

    pub fn add_view(&mut self, pass: GraphPassId, view: RenderView) {
        self.by_pass.entry(pass).or_default().push(view);
    }

    pub fn views_for(&self, pass: GraphPassId) -> &[RenderView] {
        self.by_pass.get(&pass).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// Connects producer-owned data to a graph pass, material variant, ordering phase and view class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderRoute {
    pub graph_pass: GraphPassId,
    pub material_pass: MaterialPassId,
    pub phase: RenderPhaseId,
    pub views: RenderViewSelector,
}

pub struct ProducerPrepareContext<'a> {
    pub resources: &'a mut RenderResources,
    pub api: &'a mut dyn RendererAPI,
}

pub struct RenderPassContext<'a> {
    pub route: &'a RenderRoute,
    pub view: &'a RenderView,
}

/// Implement this for GPU-owned workloads such as galaxies, voxel chunks or particles.
/// The producer owns its buffers and update policy; the renderer only schedules preparation
/// and asks it to record on matching pass/view routes.
pub trait RenderProducer: Any + Send + Sync {
    fn id(&self) -> RenderProducerId;
    fn routes(&self) -> &[RenderRoute];

    /// Human-readable profiler label supplied automatically by the concrete producer type.
    fn profile_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    fn prepare_frame(&mut self, _ctx: &mut ProducerPrepareContext<'_>) {}

    fn prepare_views(
        &mut self,
        _views: &RenderViewRegistry,
        _ctx: &mut ProducerPrepareContext<'_>,
    ) {
    }

    fn record(
        &self,
        context: &mut dyn RenderContext,
        resources: &RenderResources,
        pass: &RenderPassContext<'_>,
    );

    fn as_any_mut(&mut self) -> &mut dyn Any;
}

#[derive(Default)]
pub struct RenderProducerRegistry {
    producers: Vec<Box<dyn RenderProducer>>,
    indices: HashMap<RenderProducerId, usize>,
}

impl RenderProducerRegistry {
    pub fn register(&mut self, producer: impl RenderProducer + 'static) -> Result<(), String> {
        let id = producer.id();
        if self.indices.contains_key(&id) {
            return Err(format!("render producer {id} is already registered"));
        }
        self.indices.insert(id, self.producers.len());
        self.producers.push(Box::new(producer));
        Ok(())
    }

    pub fn remove(&mut self, id: RenderProducerId) -> bool {
        let Some(index) = self.indices.remove(&id) else {
            return false;
        };
        self.producers.swap_remove(index);
        if let Some(moved) = self.producers.get(index) {
            self.indices.insert(moved.id(), index);
        }
        true
    }

    pub fn get_mut<T: 'static>(&mut self, id: RenderProducerId) -> Option<&mut T> {
        let index = *self.indices.get(&id)?;
        self.producers[index].as_any_mut().downcast_mut()
    }

    pub fn prepare(
        &mut self,
        views: &RenderViewRegistry,
        resources: &mut RenderResources,
        api: &mut dyn RendererAPI,
    ) {
        for producer in &mut self.producers {
            let mut ctx = ProducerPrepareContext { resources, api };
            {
                crate::profile_dynamic_scope!(
                    "render.producer.prepare_frame",
                    format!("render.producer.prepare_frame.{}", producer.profile_name())
                );
                producer.prepare_frame(&mut ctx);
            }
            {
                crate::profile_dynamic_scope!(
                    "render.producer.prepare_views",
                    format!("render.producer.prepare_views.{}", producer.profile_name())
                );
                producer.prepare_views(views, &mut ctx);
            }
        }
    }

    pub fn record_pass(
        &self,
        graph_pass: GraphPassId,
        views: &RenderViewRegistry,
        context: &mut dyn RenderContext,
        resources: &RenderResources,
    ) {
        for view in views.views_for(graph_pass) {
            for producer in &self.producers {
                for route in producer.routes() {
                    if route.graph_pass == graph_pass && route.views.matches(view.kind) {
                        crate::profile_dynamic_scope!(
                            "render.producer.record",
                            format!("render.producer.record.{}", producer.profile_name())
                        );
                        producer.record(context, resources, &RenderPassContext { route, view });
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum PreparedDraw {
    Direct {
        vertices: u32,
        instances: u32,
    },
    Indexed {
        first_index: u32,
        index_count: u32,
        base_vertex: i32,
        instances: u32,
    },
    Indirect {
        buffer: BufferHandle,
        offset: u64,
    },
    IndexedIndirect {
        buffer: BufferHandle,
        offset: u64,
    },
}

impl PreparedDraw {
    pub fn record(self, context: &mut dyn RenderContext) {
        match self {
            Self::Direct {
                vertices,
                instances,
            } => context.draw(vertices, instances),
            Self::Indexed {
                first_index,
                index_count,
                base_vertex,
                instances,
            } => context.draw_indexed(first_index, index_count, base_vertex, instances),
            Self::Indirect { buffer, offset } => context.draw_indirect(buffer, offset),
            Self::IndexedIndirect { buffer, offset } => {
                context.draw_indexed_indirect(buffer, offset)
            }
        }
    }
}

#[derive(Clone)]
pub struct StandardDraw {
    pub mesh: Handle<MeshAsset>,
    /// Prepared pipeline variants keyed by material pass. One retained object can therefore
    /// be recorded by forward, depth and shadow routes without duplicating object state.
    pub pipelines: Vec<PipelineOverride>,
    pub transform_index: u32,
    pub extra_bind_groups: Vec<(u32, BindGroupHandle)>,
}

/// Default retained-object producer. Custom high-volume renderers do not need to use this type.
pub struct StandardMeshProducer {
    routes: Vec<RenderRoute>,
    pub draws: Vec<StandardDraw>,
    pub transforms: Vec<TransformInstance>,
    pub database_revision: u64,
    transform_buffer: Option<BufferHandle>,
    transform_capacity: u32,
    dirty_ranges: Vec<Range<usize>>,
    full_upload: bool,
}

impl StandardMeshProducer {
    pub fn new(route: RenderRoute) -> Self {
        Self {
            routes: vec![route],
            draws: Vec::new(),
            transforms: Vec::new(),
            database_revision: 0,
            transform_buffer: None,
            transform_capacity: 0,
            dirty_ranges: Vec::new(),
            full_upload: true,
        }
    }

    pub fn add_route(&mut self, route: RenderRoute) {
        if !self.routes.contains(&route) {
            self.routes.push(route);
        }
    }

    pub fn replace(
        &mut self,
        draws: Vec<StandardDraw>,
        transforms: Vec<TransformInstance>,
        revision: u64,
    ) {
        self.draws = draws;
        self.transforms = transforms;
        self.database_revision = revision;
        self.full_upload = true;
        self.dirty_ranges.clear();
    }

    pub fn update_transforms(
        &mut self,
        updates: impl IntoIterator<Item = (Range<usize>, Vec<TransformInstance>)>,
    ) {
        for (range, values) in updates {
            if range.end <= self.transforms.len() && values.len() == range.len() {
                self.transforms[range.clone()].copy_from_slice(&values);
                self.dirty_ranges.push(range);
            }
        }
    }
}

impl RenderProducer for StandardMeshProducer {
    fn id(&self) -> RenderProducerId {
        crate::renderer::producers::STANDARD_MESHES
    }
    fn routes(&self) -> &[RenderRoute] {
        &self.routes
    }

    fn prepare_frame(&mut self, ctx: &mut ProducerPrepareContext<'_>) {
        if self.transforms.is_empty() {
            return;
        }
        let needed = self.transforms.len() as u32;
        if self.transform_buffer.is_none() || needed > self.transform_capacity {
            self.transform_capacity = needed.next_power_of_two().max(64);
            self.transform_buffer = Some(ctx.api.create_buffer(&BufferDescriptor {
                label: "standard_mesh_transforms".into(),
                size: self.transform_capacity as u64 * size_of::<TransformInstance>() as u64,
                usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            }));
            self.full_upload = true;
        }
        let buffer = self.transform_buffer.unwrap();
        if self.full_upload {
            ctx.api
                .write_buffer(buffer, bytemuck::cast_slice(&self.transforms));
        } else {
            let stride = size_of::<TransformInstance>() as u64;
            for range in self.dirty_ranges.drain(..) {
                ctx.api.write_buffer_at(
                    buffer,
                    range.start as u64 * stride,
                    bytemuck::cast_slice(&self.transforms[range]),
                );
            }
        }
        self.full_upload = false;
        self.dirty_ranges.clear();
    }

    fn record(
        &self,
        context: &mut dyn RenderContext,
        _resources: &RenderResources,
        pass: &RenderPassContext<'_>,
    ) {
        let Some(transforms) = self.transform_buffer else {
            return;
        };
        let stride = size_of::<TransformInstance>() as u64;
        let mut last_vertex = None;
        let mut last_index = None;
        if let Some(view_bind_group) = pass.view.view_bind_group {
            context.bind_bind_group(0, view_bind_group);
        }
        for draw in &self.draws {
            let Some(pipeline) = draw.pipelines.iter().find_map(|variant| {
                (variant.material_pass == pass.route.material_pass).then_some(variant.pipeline)
            }) else {
                continue;
            };
            context.bind_pipeline(pipeline);
            for &(group, binding) in &draw.extra_bind_groups {
                context.bind_bind_group(group, binding);
            }
            let Some(mesh) = context.get_mesh_binding(&draw.mesh) else {
                continue;
            };
            if last_vertex != Some(mesh.vertex_buffer) {
                context.bind_vertex_buffer(0, mesh.vertex_buffer);
                last_vertex = Some(mesh.vertex_buffer);
            }
            if last_index != Some(mesh.index_buffer) {
                context.bind_index_buffer(mesh.index_buffer);
                last_index = Some(mesh.index_buffer);
            }
            context.bind_vertex_buffer_range(
                1,
                transforms,
                draw.transform_index as u64 * stride,
                stride,
            );
            let range = mesh.draw_range;
            context.draw_indexed(range.first_index, range.index_count, range.base_vertex, 1);
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_selectors_keep_main_and_shadow_work_separate() {
        assert!(RenderViewSelector::Main.matches(RenderViewKind::Main));
        assert!(!RenderViewSelector::Main.matches(RenderViewKind::ShadowCascade { cascade: 0 }));
        assert!(
            RenderViewSelector::ShadowCascades
                .matches(RenderViewKind::ShadowCascade { cascade: 3 })
        );
    }

    #[test]
    fn view_registry_supports_multiple_views_per_pass() {
        let mut views = RenderViewRegistry::default();
        let pass = GraphPassId::new("test.shadows");
        views.add_view(
            pass,
            RenderView {
                id: RenderViewId::new("test.cascade.0"),
                kind: RenderViewKind::ShadowCascade { cascade: 0 },
                view_bind_group: None,
            },
        );
        views.add_view(
            pass,
            RenderView {
                id: RenderViewId::new("test.cascade.1"),
                kind: RenderViewKind::ShadowCascade { cascade: 1 },
                view_bind_group: None,
            },
        );
        assert_eq!(views.views_for(pass).len(), 2);
    }
}

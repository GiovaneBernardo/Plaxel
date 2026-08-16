use std::collections::{HashMap, HashSet};

use crate::{
    model::{self, Vertex},
    prelude::*,
    renderer::{
        AtmospherePassNode, DebugPassNode, DefaultMeshes, GeometryPassNode, ShadowPassNode,
        TakenRenderNode,
    },
};

pub struct GraphResources {
    pub textures: HashMap<&'static str, TextureHandle>,
    pub buffers: HashMap<&'static str, BufferHandle>,
}

impl GraphResources {
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
            buffers: HashMap::new(),
        }
    }
    pub fn resolve_inputs(
        &self,
        desc: &RenderNodeDescriptor,
    ) -> HashMap<&'static str, TextureHandle> {
        desc.input_textures
            .iter()
            .map(|&name| (name, self.textures[name]))
            .collect()
    }

    pub fn resolve_outputs(
        &self,
        desc: &RenderNodeDescriptor,
    ) -> HashMap<&'static str, TextureHandle> {
        desc.output_textures
            .iter()
            .filter_map(|output| {
                let name = match output {
                    OutputTexture::Create(slot) => slot.name,
                    OutputTexture::WriteTo(slot_name) => slot_name,
                };

                self.textures.get(name).map(|texture| (name, *texture))
            })
            .collect()
    }

    pub fn texture(&self, name: &str) -> Option<&TextureHandle> {
        self.textures.get(name)
    }

    pub fn texture_mut(&mut self, name: &str) -> Option<&mut TextureHandle> {
        self.textures.get_mut(name)
    }

    pub fn buffer(&self, name: &str) -> Option<&BufferHandle> {
        self.buffers.get(name)
    }

    pub fn buffer_mut(&mut self, name: &str) -> Option<&mut BufferHandle> {
        self.buffers.get_mut(name)
    }
}

impl RenderGraph {
    pub fn default_render_graph(default_meshes: DefaultMeshes) -> Self {
        let mut graph = RenderGraph {
            nodes: Vec::new(),
            resources: GraphResources::new(),
            compiled: false,
            disabled_nodes: HashSet::new(),
        };

        graph
            .nodes
            .push((graph_passes::SHADOWS, Box::new(ShadowPassNode::new())));

        let geometry_pass_node = GeometryPassNode {
            camera_bind_group: None,
            camera_bind_group_layout: None,
            pass_inputs_group: None,
        };
        graph.nodes.push((
            crate::renderer::ids::graph_passes::GEOMETRY,
            Box::new(geometry_pass_node),
        ));

        graph.nodes.push((
            crate::renderer::ids::graph_passes::ATMOSPHERE,
            Box::new(AtmospherePassNode::new()),
        ));

        RenderGraph::default_debug_nodes(&mut graph, default_meshes);

        graph
    }

    pub fn default_debug_nodes(graph: &mut RenderGraph, default_meshes: DefaultMeshes) {
        let vertex_layout = model::ModelVertex::layout();

        let instance_layout = VertexLayout {
            stride: std::mem::size_of::<[[f32; 4]; 5]>() as u64,
            step_mode: StepMode::Instance,
            attributes: vec![
                VertexAttribute {
                    offset: 0,
                    shader_location: 5,
                    format: AttributeFormat::Float32x4,
                },
                VertexAttribute {
                    offset: std::mem::size_of::<[f32; 4]>() as u64,
                    shader_location: 6,
                    format: AttributeFormat::Float32x4,
                },
                VertexAttribute {
                    offset: std::mem::size_of::<[f32; 8]>() as u64,
                    shader_location: 7,
                    format: AttributeFormat::Float32x4,
                },
                VertexAttribute {
                    offset: std::mem::size_of::<[f32; 12]>() as u64,
                    shader_location: 8,
                    format: AttributeFormat::Float32x4,
                },
                VertexAttribute {
                    offset: std::mem::size_of::<[f32; 16]>() as u64,
                    shader_location: 9,
                    format: AttributeFormat::Float32x4,
                },
            ],
        };

        let sphere_material = Material::new("shaders/debug.wgsl".to_string())
            .with_vertex_layouts(vec![vertex_layout.clone(), instance_layout.clone()]);
        let cube_material = sphere_material.clone();
        let wire_cube_material = Material::new("shaders/debug.wgsl".to_string())
            .with_vertex_layouts(vec![vertex_layout.clone(), instance_layout.clone()])
            .with_topology(Topology::LineList);

        let debug_pass_node = DebugPassNode {
            camera_buffer: None,
            camera_bind_group: None,
            camera_bind_group_layout: None,
            pass_inputs_group: None,
            cubes: Vec::new(),
            wire_cubes: Vec::new(),
            sphere_positions: Vec::new(),
            sphere_mesh: default_meshes.sphere,
            sphere_material,
            cube_mesh: default_meshes.cube,
            cube_material,
            wire_cube_mesh: default_meshes.wire_cube,
            wire_cube_material,
            sphere_instance_buffer: None,
            cube_instance_buffer: None,
            wire_cube_instance_buffer: None,
            sphere_instance_capacity: 0,
            cube_instance_capacity: 0,
            wire_cube_instance_capacity: 0,
            sphere_instance_count: 0,
            cube_instance_count: 0,
            wire_cube_instance_count: 0,
        };

        graph.nodes.push((
            crate::renderer::ids::graph_passes::DEBUG,
            Box::new(debug_pass_node),
        ));
    }

    fn allocate_graph_resources(
        nodes: &Vec<(GraphPassId, Box<dyn RenderNode>)>,
        api: &mut dyn RendererAPI,
    ) -> GraphResources {
        let mut textures = HashMap::new();
        let mut buffers = HashMap::new();

        for (_, node) in nodes {
            for slot in node.describe_pass().output_textures {
                match slot {
                    OutputTexture::Create(slot) => {
                        textures.insert(slot.name, api.create_texture(&slot.texture_descriptor));
                    }
                    _ => {}
                }
            }

            for slot in node.describe_pass().output_buffers {
                match slot {
                    OutputBuffer::Create(slot) => {
                        buffers.insert(slot.name, api.create_buffer(&slot.buffer_descriptor));
                    }
                    _ => {}
                }
            }
        }

        GraphResources { textures, buffers }
    }

    pub fn compile(&mut self, render_resources: &mut RenderResources, api: &mut dyn RendererAPI) {
        self.resources = RenderGraph::allocate_graph_resources(&self.nodes, api); // textures for all declared outputs

        for (_, node) in &mut self.nodes {
            let desc = node.describe_pass();
            let target_info = api.target_info_for_pass(&desc, &self.resources);
            let mut ctx = NodeCompileContext {
                api,
                render_resources,
                resolved_inputs: self.resources.resolve_inputs(&desc),
                resolved_outputs: self.resources.resolve_outputs(&desc),
                target_info,
            };
            node.compile(&mut ctx);
        }
        self.compiled = true;
    }

    pub fn resize(
        &mut self,
        api: &mut dyn RendererAPI,
        render_resources: &mut RenderResources,
        width: u32,
        height: u32,
    ) {
        for (_, node) in &mut self.nodes {
            let desc = node.describe_pass();
            let target_info = api.target_info_for_pass(&desc, &self.resources);
            let mut ctx = NodeCompileContext {
                api,
                render_resources,
                resolved_inputs: self.resources.resolve_inputs(&desc),
                resolved_outputs: self.resources.resolve_outputs(&desc),
                target_info,
            };

            node.resize(&mut ctx, &self.resources, width, height);
        }
    }

    pub fn get_node_mut<T: 'static>(&mut self, index: GraphPassId) -> Option<&mut T> {
        for (node_index, node) in &mut self.nodes {
            if *node_index == index {
                return node.as_any_mut().downcast_mut::<T>();
            }
        }
        None
    }

    /// Remove a node from the graph while remembering its execution position.
    /// Use `return_node` to put it back after you're done.
    /// This is useful to avoid borrow conflicts when the node needs
    /// mutable access to State while being part of State.
    pub fn take_node(&mut self, index: GraphPassId) -> Option<TakenRenderNode> {
        if let Some(pos) = self.nodes.iter().position(|(i, _)| *i == index) {
            let (id, node) = self.nodes.remove(pos);
            Some(TakenRenderNode {
                id,
                position: pos,
                node,
            })
        } else {
            None
        }
    }

    /// Return a previously taken node to its original execution position.
    pub fn return_node(&mut self, taken: TakenRenderNode) {
        let position = taken.position.min(self.nodes.len());
        self.nodes.insert(position, (taken.id, taken.node));
    }

    pub fn is_node_enabled(&self, index: GraphPassId) -> bool {
        !self.disabled_nodes.contains(&index)
    }

    pub fn set_node_enabled(&mut self, index: GraphPassId, enabled: bool) {
        if enabled {
            self.disabled_nodes.remove(&index);
        } else {
            self.disabled_nodes.insert(index);
        }
    }
}

impl dyn RenderNode {}

use cgmath::{Point3, vec3};
use engine::{core::physics::physics::Physics, ecs::world::World, renderer::DebugPassNode};
use game_types::planet::Planet;

use crate::octree::{self, depth_color};

pub fn sync_planet_debug(
    renderer: &mut engine::renderer::Renderer,
    _world: &World,
    planet: &Planet,
) {
    let Some(debug_pass_node) = renderer.render_graph.get_node_mut::<DebugPassNode>(2) else {
        return;
    };

    let mut out = Vec::<(Point3<f32>, f32, u32)>::new();

    octree::collect_octree_nodes(&planet.octree_root, 0, &mut out);
    for (center, size, depth) in out.iter() {
        debug_pass_node.add_wire_cube(*center, *size, depth_color(*depth));
        debug_pass_node.add_cube(
            *center + vec3(0.0, size / 2.0, 0.0),
            1.0,
            depth_color(depth + 1),
        );
    }
}

pub fn sync_physics_debug(
    renderer: &mut engine::renderer::Renderer,
    world: &World,
    planet: &Planet,
) {
    let Some(physics) = world.get_resource::<Physics>() else {
        return;
    };
    let Some(debug_pass_node) = renderer.render_graph.get_node_mut::<DebugPassNode>(2) else {
        return;
    };

    //debug_pass_node.clear_spheres();
    //for (_, body) in physics.rigid_body_set.iter() {
    //    let position = body.translation();
    //    debug_pass_node.add_sphere(Point3::new(position.x, position.y, position.z));
    //}
}

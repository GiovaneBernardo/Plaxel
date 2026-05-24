use cgmath::point3;
use egui::{Pos2, Response};
use engine::{
    ecs::{commands::PhysicalSphereParams, system::SystemContext},
    renderer::GeometryPassNode,
};

#[derive(Default)]
pub struct EditorSpawnRequests {
    pub physical_spheres: Vec<PhysicalSphereParams>,
}

pub fn editor_spawn_system(
    ctx: &mut SystemContext,
    commands: &mut engine::ecs::commands::Commands,
) {
    let Some(mut requests) = ctx.world.get_resource_mut::<EditorSpawnRequests>() else {
        return;
    };

    let physical_spheres = std::mem::take(&mut requests.physical_spheres);
    drop(requests);

    for params in physical_spheres {
        commands.spawn_physical_sphere(params);
    }
}

#[unsafe(no_mangle)]
pub fn viewport_context_menu(
    viewport_menu_pos: &mut egui::Pos2,
    viewport_menu_open: &mut bool,
    state: &mut engine::State,
    ctx: &egui::Context,
) -> Response {
    egui::Area::new(egui::Id::new("viewport_context_menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(*viewport_menu_pos)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                if ui.button("Spawn cube").clicked() {
                    *viewport_menu_open = false;
                    let world_pos = get_world_pos(state, &viewport_menu_pos);

                    let Some(scene) = state.active_scene_mut() else {
                        return;
                    };
                    let Some(mut requests) =
                        scene.world_mut().get_resource_mut::<EditorSpawnRequests>()
                    else {
                        return;
                    };

                    requests.physical_spheres.push(PhysicalSphereParams {
                        mass: 50.0,
                        position: cgmath::vec3(world_pos.x, world_pos.y, world_pos.z),
                        radius: 0.5,
                    });
                }
            });
        })
        .response
}

pub fn get_world_pos(state: &mut engine::State, mouse_pos: &Pos2) -> cgmath::Point3<f32> {
    let Some(mut node) = state.global_resources.renderer.render_graph.take_node(0) else {
        return point3(0.0, 0.0, 0.0);
    };

    let mut world_pos = point3(0.0, 0.0, 0.0);

    if let Some(geometry_pass_node) = node.as_any_mut().downcast_mut::<GeometryPassNode>() {
        world_pos = geometry_pass_node.get_world_position_from_depth(
            state.global_resources.renderer.renderer_api.as_mut(),
            &mut state.global_resources.renderer.render_graph.resources,
            &mut state.global_resources.renderer.render_resources,
            mouse_pos.x,
            mouse_pos.y,
        );
    }

    println!("Position: {:?}", world_pos);
    state
        .global_resources
        .renderer
        .render_graph
        .return_node(0, node);

    world_pos
}

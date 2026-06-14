use cgmath::vec3;
use engine::{core::components::core::TransformComponent, ecs::query::Query};

#[unsafe(no_mangle)]
pub fn hierarchy_draw(state: &mut engine::State, ctx: &egui::Context) {
    egui::Window::new("Hierarchy")
        .resizable([true, true])
        .default_size([2000.0, 1000.0])
        .show(ctx, |ui| {
            ui.label("Hierarchy");
            if ui.button("Click me").clicked() {}

            if ui.button("New Entity").clicked() {
                state.active_scene_mut().unwrap().world_mut().spawn();
            }

            let scene = state.active_scene_mut().unwrap();
            let world = scene.world_mut();
            if ui.button("Spawn 1000").clicked() {
                for _i in 0..1000 {
                    let entity = world.spawn();
                    world.insert(
                        entity,
                        TransformComponent {
                            position: vec3(0.0, 10.0, 0.0),
                            rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
                            scale: vec3(1.0, 1.0, 1.0),
                            velocity: vec3(0.0, 0.0, 0.0),
                        },
                    );
                }
            }

            let entities: Vec<_> = world.entities().iter_alive().collect();
            for entity in entities {
                egui::CollapsingHeader::new(entity.index().to_string())
                    .default_open(true)
                    .show(ui, |ui| {
                        if ui.button("Add Transform").clicked() {
                            world.insert(
                                entity,
                                TransformComponent {
                                    position: vec3(0.0, 10.0, 0.0),
                                    rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
                                    scale: vec3(1.0, 1.0, 1.0),
                                    velocity: vec3(0.0, 0.0, 0.0),
                                },
                            );
                        }

                        if ui.button("Add StaticMeshRenderer").clicked() {}
                        if ui.button("Add DynamicMeshRenderer").clicked() {}
                    })
                    .body_returned;
            }

            //let mut query = TransformQuery::new(world);
            let mut query = Query::<(&mut TransformComponent,)>::new(world);
            //println!(query.iter_size());
            query.for_each(|_entity, (transform,)| {
                ui.add(egui::Label::new("Position"));

                ui.add(egui::widgets::DragValue::new(&mut transform.position.x));
                ui.add(egui::widgets::DragValue::new(&mut transform.position.y));
                ui.add(egui::widgets::DragValue::new(&mut transform.position.z));
            });

            // ui.add(egui::Label::new("Position"));
            // ui.add(egui::widgets::DragValue::new(&mut state.camera.position.x));
            // ui.add(egui::widgets::DragValue::new(&mut state.camera.position.y));
            // ui.add(egui::widgets::DragValue::new(&mut state.camera.position.z));

            // let mut i = 0;
            // for transform in &mut state.scene.transform_components {
            //     egui::CollapsingHeader::new(i.to_string())
            //         .default_open(true)
            //         .show(ui, |ui| {
            //             ui.add(egui::Label::new("Position"));
            //             ui.add(egui::widgets::DragValue::new(&mut transform.position.x));
            //             ui.add(egui::widgets::DragValue::new(&mut transform.position.y));
            //             ui.add(egui::widgets::DragValue::new(&mut transform.position.z));

            //             let mut euler: cgmath::Euler<cgmath::Rad<f32>> =
            //                 cgmath::Euler::from(transform.rotation);
            //             let mut rot_deg = cgmath::Vector3::new(
            //                 euler.x.0.to_degrees(),
            //                 euler.y.0.to_degrees(),
            //                 euler.z.0.to_degrees(),
            //             );

            //             ui.label("Rotation (World)");
            //             ui.add(egui::DragValue::new(&mut rot_deg.x));
            //             ui.add(egui::DragValue::new(&mut rot_deg.y));
            //             ui.add(egui::DragValue::new(&mut rot_deg.z));

            //             let rot_rad = cgmath::Vector3::new(
            //                 rot_deg.x.to_radians(),
            //                 rot_deg.y.to_radians(),
            //                 rot_deg.z.to_radians(),
            //             );

            //             let new_euler = cgmath::Euler {
            //                 x: cgmath::Rad(rot_rad.x),
            //                 y: cgmath::Rad(rot_rad.y),
            //                 z: cgmath::Rad(rot_rad.z),
            //             };

            //             transform.rotation = cgmath::Quaternion::from(new_euler);

            //             ui.label("Scaleleuio");
            //             ui.add(egui::widgets::DragValue::new(&mut transform.scale.x));
            //             ui.add(egui::widgets::DragValue::new(&mut transform.scale.y));
            //             ui.add(egui::widgets::DragValue::new(&mut transform.scale.z));
            //         });
            //     i += 1;
            // }
        });
}

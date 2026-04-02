#[unsafe(no_mangle)]
pub fn hierarchy_draw(state: &mut engine::State, ctx: &egui::Context) {
    egui::Window::new("Hierarchy")
        .resizable([true, true])
        .default_size([2000.0, 1000.0])
        .show(ctx, |ui| {
            ui.label("Hierarchy");
            if ui.button("Click me").clicked() {}

            let mut i = 0;
            for transform in &mut state.scene.transform_components {
                egui::CollapsingHeader::new(i.to_string())
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.add(egui::Label::new("Position"));
                        ui.add(egui::widgets::DragValue::new(&mut transform.position.x));
                        ui.add(egui::widgets::DragValue::new(&mut transform.position.y));
                        ui.add(egui::widgets::DragValue::new(&mut transform.position.z));

                        let mut euler: cgmath::Euler<cgmath::Rad<f32>> =
                            cgmath::Euler::from(transform.rotation);
                        let mut rot_deg = cgmath::Vector3::new(
                            euler.x.0.to_degrees(),
                            euler.y.0.to_degrees(),
                            euler.z.0.to_degrees(),
                        );

                        ui.label("Rotation (World)");
                        ui.add(egui::DragValue::new(&mut rot_deg.x));
                        ui.add(egui::DragValue::new(&mut rot_deg.y));
                        ui.add(egui::DragValue::new(&mut rot_deg.z));

                        let rot_rad = cgmath::Vector3::new(
                            rot_deg.x.to_radians(),
                            rot_deg.y.to_radians(),
                            rot_deg.z.to_radians(),
                        );

                        let new_euler = cgmath::Euler {
                            x: cgmath::Rad(rot_rad.x),
                            y: cgmath::Rad(rot_rad.y),
                            z: cgmath::Rad(rot_rad.z),
                        };

                        transform.rotation = cgmath::Quaternion::from(new_euler);

                        ui.label("Scaleleuio");
                        ui.add(egui::widgets::DragValue::new(&mut transform.scale.x));
                        ui.add(egui::widgets::DragValue::new(&mut transform.scale.y));
                        ui.add(egui::widgets::DragValue::new(&mut transform.scale.z));
                    });
                i += 1;
            }
        });
}

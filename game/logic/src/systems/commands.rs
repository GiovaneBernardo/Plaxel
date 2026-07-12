// Currently leaving it as an example as I'll clearly forget how to expand the commands
// This is a very good example on how to do so but it'll be moved to engine
// TODO: Remember to delete as soon as this file has another command that can be used as an example

//pub trait GameCommandsExt {
//    fn spawn_physical_sphere(&mut self, params: PhysicalSphereParams);
//}
//pub struct PhysicalSphereParams {
//    pub position: engine::math::Vec3,
//    pub radius: f32,
//    pub mass: f32,
//}//

//impl GameCommandsExt for Commands {
//    fn spawn_physical_sphere(&mut self, params: PhysicalSphereParams) {
//        self.push(move |world: &mut World| {
//            let entity = world.spawn();
//            world.insert(
//                entity,
//                TransformComponent {
//                    position: params.position,
//                    rotation: crate::math::Quat::IDENTITY,
//                    scale: engine::math::vec3(params.radius, params.radius, params.radius),
//                    velocity: engine::math::vec3(0.0, 0.0, 0.0),
//                },
//            );
//            world.insert(
//                entity,
//                ColliderComponent {
//                    shape: ColliderShape::Sphere {
//                        radius: params.radius,
//                    },
//                    friction: 0.5,
//                    restitution: 0.5,
//                },
//            );
//            world.insert(
//                entity,
//                RigidBodyComponent {
//                    kind: BodyKind::Dynamic,
//                    mass: params.mass,
//                    velocity: engine::math::vec3(0.0, 0.0, 0.0),
//                },
//            );
//        });
//    }
//}

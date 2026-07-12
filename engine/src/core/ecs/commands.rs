use crate::{
    core::components::{
        core::TransformComponent,
        physics::{BodyKind, ColliderComponent, ColliderShape, RigidBodyComponent},
    },
    ecs::{component::Component, entity::Entity, system::SystemContext},
};

pub trait Command {
    fn apply(self: Box<Self>, ctx: &mut SystemContext);
}

impl<F> Command for F
where
    F: FnOnce(&mut SystemContext),
{
    fn apply(self: Box<Self>, ctx: &mut SystemContext) {
        self(ctx);
    }
}

pub struct Commands {
    queue: Vec<Box<dyn Command>>,
}

pub struct PhysicalSphereParams {
    pub position: crate::math::Vec3,
    pub radius: f32,
    pub mass: f32,
}

impl Commands {
    pub fn new() -> Self {
        Self { queue: Vec::new() }
    }

    pub fn push(&mut self, command: impl FnOnce(&mut SystemContext) + 'static) {
        self.queue.push(Box::new(command));
    }

    pub fn spawn(&mut self) {
        self.push(|ctx: &mut SystemContext| {
            ctx.world.spawn();
        });
    }

    pub fn insert<T: Component>(&mut self, entity: Entity, component: T) {
        self.push(move |ctx: &mut SystemContext| {
            ctx.world.insert(entity, component);
        });
    }

    pub fn apply(&mut self, ctx: &mut SystemContext) {
        for command in self.queue.drain(..) {
            command.apply(ctx);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn spawn_physical_sphere(&mut self, params: PhysicalSphereParams) {
        self.push(move |ctx: &mut SystemContext| {
            let entity = ctx.world.spawn();
            ctx.world.insert(
                entity,
                TransformComponent {
                    position: params.position,
                    rotation: crate::math::Quat::IDENTITY,
                    scale: crate::math::vec3(params.radius, params.radius, params.radius),
                    velocity: crate::math::vec3(0.0, 0.0, 0.0),
                },
            );
            ctx.world.insert(
                entity,
                ColliderComponent {
                    shape: ColliderShape::Sphere {
                        radius: params.radius,
                    },
                    friction: 0.5,
                    restitution: 0.5,
                },
            );
            ctx.world.insert(
                entity,
                RigidBodyComponent {
                    kind: BodyKind::Dynamic,
                    mass: params.mass,
                    velocity: crate::math::vec3(0.0, 0.0, 0.0),
                },
            );
        });
    }
}

impl Default for Commands {
    fn default() -> Self {
        Self::new()
    }
}

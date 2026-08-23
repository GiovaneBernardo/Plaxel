use crate::reflect::Reflect;
use crate::{
    core::components::{
        core::TransformComponent,
        physics::{BodyKind, ColliderComponent, ColliderShape, RigidBodyComponent},
    },
    ecs::{
        component::Component,
        entity::{Entity, EntityAllocator},
        resource::Resource,
        system::SystemContext,
        world::World,
    },
};

pub trait Command: Send {
    fn apply(self: Box<Self>, ctx: &mut SystemContext);
}

impl<F> Command for F
where
    F: FnOnce(&mut SystemContext) + Send,
{
    fn apply(self: Box<Self>, ctx: &mut SystemContext) {
        self(ctx);
    }
}

pub struct Commands {
    queue: Vec<Box<dyn Command>>,
    entity_allocator: Option<EntityAllocator>,
}

/// A collection of components that can be inserted together on an entity.
pub trait Bundle: Send + 'static {
    fn insert(self, world: &mut World, entity: Entity);
}

macro_rules! impl_bundle {
    ($(($component:ident, $value:ident)),+) => {
        impl<$($component),+> Bundle for ($($component,)+)
        where
            $($component: Component + Reflect,)+
        {
            fn insert(self, world: &mut World, entity: Entity) {
                let ($($value,)+) = self;
                $(world.insert(entity, $value);)+
            }
        }
    };
}

macro_rules! impl_bundle_tuples {
    (($component:ident, $value:ident)) => {
        impl_bundle!(($component, $value));
    };
    (($component:ident, $value:ident), $(($rest_component:ident, $rest_value:ident)),+) => {
        impl_bundle!(
            ($component, $value),
            $(($rest_component, $rest_value)),+
        );
        impl_bundle_tuples!($(($rest_component, $rest_value)),+);
    };
}

impl_bundle_tuples!(
    (C0, c0),
    (C1, c1),
    (C2, c2),
    (C3, c3),
    (C4, c4),
    (C5, c5),
    (C6, c6),
    (C7, c7),
    (C8, c8),
    (C9, c9),
    (C10, c10),
    (C11, c11),
    (C12, c12),
    (C13, c13),
    (C14, c14),
    (C15, c15)
);

/// Fluent access to commands targeting one entity.
pub struct EntityCommands<'a> {
    entity: Entity,
    commands: &'a mut Commands,
}

impl EntityCommands<'_> {
    pub fn id(&self) -> Entity {
        self.entity
    }

    pub fn insert<T: Component + Reflect>(&mut self, component: T) -> &mut Self {
        self.commands.insert(self.entity, component);
        self
    }

    pub fn insert_bundle<B: Bundle>(&mut self, bundle: B) -> &mut Self {
        self.commands.insert_bundle(self.entity, bundle);
        self
    }

    pub fn despawn(&mut self) -> &mut Self {
        self.commands.despawn(self.entity);
        self
    }
}

#[derive(plaxel_reflect::Reflect)]
pub struct PhysicalSphereParams {
    pub position: crate::math::Vec3,
    pub radius: f32,
    pub mass: f32,
}

impl Commands {
    pub fn new() -> Self {
        Self {
            queue: Vec::new(),
            entity_allocator: None,
        }
    }

    pub(crate) fn for_world(world: &World) -> Self {
        Self {
            queue: Vec::new(),
            entity_allocator: Some(world.entity_allocator()),
        }
    }

    pub(crate) fn attach_world(&mut self, world: &World) {
        self.entity_allocator = Some(world.entity_allocator());
    }

    pub fn push(&mut self, command: impl FnOnce(&mut SystemContext) + Send + 'static) {
        self.queue.push(Box::new(command));
    }

    pub fn spawn<B: Bundle>(&mut self, bundle: B) -> EntityCommands<'_> {
        let entity = self.reserve_entity();
        self.push(move |ctx: &mut SystemContext| {
            ctx.world.spawn_reserved(entity);
            bundle.insert(ctx.world, entity);
        });
        EntityCommands {
            entity,
            commands: self,
        }
    }

    pub fn spawn_empty(&mut self) -> EntityCommands<'_> {
        let entity = self.reserve_entity();
        self.push(move |ctx: &mut SystemContext| {
            ctx.world.spawn_reserved(entity);
        });
        EntityCommands {
            entity,
            commands: self,
        }
    }

    pub fn entity(&mut self, entity: Entity) -> EntityCommands<'_> {
        EntityCommands {
            entity,
            commands: self,
        }
    }

    pub fn insert<T: Component + Reflect>(&mut self, entity: Entity, component: T) {
        self.push(move |ctx: &mut SystemContext| {
            ctx.world.insert(entity, component);
        });
    }

    pub fn insert_bundle<B: Bundle>(&mut self, entity: Entity, bundle: B) {
        self.push(move |ctx: &mut SystemContext| {
            bundle.insert(ctx.world, entity);
        });
    }

    pub fn despawn(&mut self, entity: Entity) {
        self.push(move |ctx: &mut SystemContext| {
            ctx.world.despawn(entity);
        });
    }

    pub fn insert_resource<T: Resource + Reflect>(&mut self, resource: T) {
        self.push(move |ctx: &mut SystemContext| {
            ctx.world.insert_resource(resource);
        });
    }

    pub fn insert_opaque_resource<T: Resource>(&mut self, resource: T) {
        self.push(move |ctx: &mut SystemContext| {
            ctx.world.insert_opaque_resource(resource);
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

    fn reserve_entity(&self) -> Entity {
        self.entity_allocator
            .as_ref()
            .expect("Commands must be initialized for a World before spawning")
            .reserve()
    }

    pub fn spawn_physical_sphere(&mut self, params: PhysicalSphereParams) {
        self.spawn((
            TransformComponent {
                position: params.position,
                rotation: crate::math::Quat::IDENTITY,
                scale: crate::math::vec3(params.radius, params.radius, params.radius),
                velocity: crate::math::vec3(0.0, 0.0, 0.0),
            },
            ColliderComponent {
                shape: ColliderShape::Sphere {
                    radius: params.radius,
                },
                friction: 0.5,
                restitution: 0.5,
            },
            RigidBodyComponent {
                kind: BodyKind::Dynamic,
                mass: params.mass,
                velocity: crate::math::vec3(0.0, 0.0, 0.0),
            },
        ));
    }
}

impl Default for Commands {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, plaxel_reflect::Reflect)]
    struct First(u32);

    #[derive(Debug, PartialEq, plaxel_reflect::Reflect)]
    struct Second(u32);

    #[test]
    fn bundles_insert_all_components() {
        let mut world = World::new();
        let entity = world.spawn();

        (First(1), Second(2)).insert(&mut world, entity);

        assert_eq!(*world.get::<First>(entity).unwrap(), First(1));
        assert_eq!(*world.get::<Second>(entity).unwrap(), Second(2));
    }

    #[test]
    fn spawning_reserves_an_id_without_making_it_visible_early() {
        let world = World::new();
        let mut commands = Commands::for_world(&world);

        let entity = commands.spawn((First(1),)).id();

        assert!(!world.entities().contains(entity));
        assert_eq!(commands.len(), 1);
    }
}

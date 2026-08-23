use std::{
    any::type_name,
    cell::{Ref, RefMut},
};

use crate::ecs::{
    access::{AccessConflict, SystemAccess},
    change::ChangeTick,
    component::Component,
    entity::Entity,
    storage::Storage,
    world::World,
};

pub trait QueryParam<'w> {
    type Fetch;

    type Item<'a>;

    fn borrow(world: &'w World) -> Option<Self::Fetch>;

    fn get<'a>(fetch: &'a mut Self::Fetch, entity: Entity) -> Option<Self::Item<'a>>;

    fn register_access(access: &mut SystemAccess) -> Result<(), AccessConflict>;
}

impl<'w, T: Component> QueryParam<'w> for &T {
    type Fetch = Ref<'w, Storage<T>>;

    type Item<'a> = &'a T;

    fn borrow(world: &'w World) -> Option<Self::Fetch> {
        world.get_storage::<T>()
    }

    fn get<'a>(fetch: &'a mut Self::Fetch, entity: Entity) -> Option<Self::Item<'a>> {
        fetch.get(entity)
    }

    fn register_access(access: &mut SystemAccess) -> Result<(), AccessConflict> {
        access.add_component_read(type_name::<T>())
    }
}

impl<'w, T: Component> QueryParam<'w> for &mut T {
    type Fetch = (RefMut<'w, Storage<T>>, ChangeTick);

    type Item<'a> = &'a mut T;

    fn borrow(world: &'w World) -> Option<Self::Fetch> {
        Some((world.get_storage_mut::<T>()?, world.change_tick()))
    }

    fn get<'a>(fetch: &'a mut Self::Fetch, entity: Entity) -> Option<Self::Item<'a>> {
        fetch.0.get_mut(entity, fetch.1)
    }

    fn register_access(access: &mut SystemAccess) -> Result<(), AccessConflict> {
        access.add_component_write(type_name::<T>())
    }
}

pub trait QueryTuple<'w> {
    type Fetch;

    type Item<'a>;

    fn borrow(world: &'w World) -> Option<Self::Fetch>;

    fn get<'a>(fetch: &'a mut Self::Fetch, entity: Entity) -> Option<Self::Item<'a>>;

    fn register_access(access: &mut SystemAccess) -> Result<(), AccessConflict>;
}

macro_rules! impl_query_tuple {
    ($($name:ident),+) => {
        impl<'w, $($name),+> QueryTuple<'w> for ($($name,)+)
        where
            $($name: QueryParam<'w>,)+
        {
            type Fetch = ($($name::Fetch,)+);

            type Item<'a> = ($($name::Item<'a>,)+);

            fn borrow(world: &'w World) -> Option<Self::Fetch> {
                Some(($($name::borrow(world)?,)+))
            }

            fn get<'a>(
                fetch: &'a mut Self::Fetch,
                entity: Entity,
            ) -> Option<Self::Item<'a>> {
                #[allow(non_snake_case)]
                let ($($name,)+) = fetch;

                Some(($($name::get($name, entity)?,)+))
            }

            fn register_access(access: &mut SystemAccess) -> Result<(), AccessConflict> {
                $($name::register_access(access)?;)+
                Ok(())
            }
        }
    };
}

pub struct Query<'w, Q>
where
    Q: QueryTuple<'w>,
{
    fetch: Option<Q::Fetch>,
    entities: Vec<Entity>,
}

impl<'w, Q> Query<'w, Q>
where
    Q: QueryTuple<'w>,
{
    pub fn new(world: &'w World) -> Self {
        Self {
            fetch: Q::borrow(world),
            entities: world.entities().iter_alive().collect(),
        }
    }

    pub fn for_each(&mut self, mut f: impl for<'a> FnMut(Entity, Q::Item<'a>)) {
        let Some(fetch) = self.fetch.as_mut() else {
            return;
        };

        for entity in self.entities.iter().copied() {
            if let Some(item) = Q::get(fetch, entity) {
                f(entity, item);
            }
        }
    }

    pub fn get(&mut self, entity: Entity) -> Option<Q::Item<'_>> {
        Q::get(self.fetch.as_mut()?, entity)
    }
}

impl_query_tuple!(A);
impl_query_tuple!(A, B);
impl_query_tuple!(A, B, C);
impl_query_tuple!(A, B, C, D);
impl_query_tuple!(A, B, C, D, E);
impl_query_tuple!(A, B, C, D, E, F);
impl_query_tuple!(A, B, C, D, E, F, G);
impl_query_tuple!(A, B, C, D, E, F, G, H);
impl_query_tuple!(A, B, C, D, E, F, G, H, I);
impl_query_tuple!(A, B, C, D, E, F, G, H, I, J);
impl_query_tuple!(A, B, C, D, E, F, G, H, I, J, K);
impl_query_tuple!(A, B, C, D, E, F, G, H, I, J, K, L);
impl_query_tuple!(A, B, C, D, E, F, G, H, I, J, K, L, M);
impl_query_tuple!(A, B, C, D, E, F, G, H, I, J, K, L, M, N);
impl_query_tuple!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O);
impl_query_tuple!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P);

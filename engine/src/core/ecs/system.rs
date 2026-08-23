use std::{
    any::type_name,
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use crate::{
    ecs::{
        access::{AccessConflict, SystemAccess},
        commands::Commands,
        event::{Event, EventReader, EventWriter, Events, ManualEventReader},
        query::{Query, QueryTuple},
        resource::{Res, ResMut, Resource},
        world::World,
    },
    global_resources::GlobalResources,
};

pub struct SystemContext<'a> {
    pub world: &'a mut World,
    pub globals: &'a mut GlobalResources,
    pub last_run_tick: crate::ecs::change::ChangeTick,
    pub this_run_tick: crate::ecs::change::ChangeTick,
}

/// A read-only parameter for the engine's current global resource aggregate.
/// Prefer typed `Res<T>` parameters for world resources. This exists as an
/// incremental bridge for renderer and platform state that still lives in
/// `GlobalResources`.
pub struct Globals<'w> {
    value: &'w GlobalResources,
}

impl Deref for Globals<'_> {
    type Target = GlobalResources;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

/// Mutable access to the engine's current global resource aggregate.
/// Systems using this parameter conflict with every other global writer.
pub struct GlobalsMut<'w> {
    value: &'w mut GlobalResources,
}

impl Deref for GlobalsMut<'_> {
    type Target = GlobalResources;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl DerefMut for GlobalsMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
    }
}

/// Per-system state initialized with `T::default()` and retained across runs.
pub struct Local<'state, T> {
    value: &'state mut T,
}

/// The change-detection ticks for the current system invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemTicks {
    pub last_run_tick: crate::ecs::change::ChangeTick,
    pub this_run_tick: crate::ecs::change::ChangeTick,
}

impl<T> Deref for Local<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<T> DerefMut for Local<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
    }
}

/// Raw access used only after `SystemAccess` has validated a parameter tuple.
///
/// The current scheduler remains sequential. This representation prevents the
/// Rust borrow checker from treating independent component/resource borrows as
/// one borrow of the entire `World`; safety is maintained by parameter access
/// validation plus the existing `RefCell` guards in world storage.
#[derive(Clone, Copy)]
pub struct UnsafeSystemContext<'world> {
    world: *mut World,
    globals: *mut GlobalResources,
    last_run_tick: crate::ecs::change::ChangeTick,
    this_run_tick: crate::ecs::change::ChangeTick,
    marker: PhantomData<&'world mut ()>,
}

impl<'world> UnsafeSystemContext<'world> {
    fn new(context: &mut SystemContext<'world>) -> Self {
        Self {
            world: context.world as *mut World,
            globals: context.globals as *mut GlobalResources,
            last_run_tick: context.last_run_tick,
            this_run_tick: context.this_run_tick,
            marker: PhantomData,
        }
    }

    /// # Safety
    ///
    /// The caller must have registered compatible read access to the world.
    pub unsafe fn world(self) -> &'world World {
        unsafe { &*self.world }
    }

    /// # Safety
    ///
    /// The caller must have registered shared global access.
    unsafe fn globals(self) -> &'world GlobalResources {
        unsafe { &*self.globals }
    }

    /// # Safety
    ///
    /// The caller must have registered unique global access.
    unsafe fn globals_mut(self) -> &'world mut GlobalResources {
        unsafe { &mut *self.globals }
    }

    #[cfg(test)]
    unsafe fn from_world(world: &'world mut World) -> Self {
        Self {
            world,
            globals: std::ptr::null_mut(),
            last_run_tick: crate::ecs::change::ChangeTick::default(),
            this_run_tick: crate::ecs::change::ChangeTick::default(),
            marker: PhantomData,
        }
    }
}

/// Describes a value that can be automatically supplied to a system function.
///
/// `State` persists for the lifetime of the system. `Item` is the value passed
/// to one invocation and may borrow both the world and the parameter state.
pub unsafe trait SystemParam: Sized + 'static {
    type State: Send + 'static;
    type Item<'world, 'state>;

    fn init(_world: &mut World) -> Self::State;

    fn register_access(access: &mut SystemAccess) -> Result<(), AccessConflict>;

    /// # Safety
    ///
    /// The complete system parameter tuple must have successfully registered
    /// its access before any values are fetched from this context.
    unsafe fn get_param<'world, 'state>(
        state: &'state mut Self::State,
        context: UnsafeSystemContext<'world>,
    ) -> Self::Item<'world, 'state>;

    fn apply(_state: &mut Self::State, _context: &mut SystemContext<'_>) {}
}

unsafe impl SystemParam for () {
    type State = ();
    type Item<'world, 'state> = ();

    fn init(_world: &mut World) -> Self::State {}

    fn register_access(_access: &mut SystemAccess) -> Result<(), AccessConflict> {
        Ok(())
    }

    unsafe fn get_param<'world, 'state>(
        _state: &'state mut Self::State,
        _context: UnsafeSystemContext<'world>,
    ) -> Self::Item<'world, 'state> {
    }
}

unsafe impl<T: Resource> SystemParam for Res<'static, T> {
    type State = ();
    type Item<'world, 'state> = Res<'world, T>;

    fn init(_world: &mut World) -> Self::State {}

    fn register_access(access: &mut SystemAccess) -> Result<(), AccessConflict> {
        access.add_resource_read(type_name::<T>())
    }

    unsafe fn get_param<'world, 'state>(
        _state: &'state mut Self::State,
        context: UnsafeSystemContext<'world>,
    ) -> Self::Item<'world, 'state> {
        let value = unsafe { context.world() }
            .get_resource::<T>()
            .unwrap_or_else(|| panic!("required resource `{}` is missing", type_name::<T>()));
        Res::new(value)
    }
}

unsafe impl<T: Resource> SystemParam for ResMut<'static, T> {
    type State = ();
    type Item<'world, 'state> = ResMut<'world, T>;

    fn init(_world: &mut World) -> Self::State {}

    fn register_access(access: &mut SystemAccess) -> Result<(), AccessConflict> {
        access.add_resource_write(type_name::<T>())
    }

    unsafe fn get_param<'world, 'state>(
        _state: &'state mut Self::State,
        context: UnsafeSystemContext<'world>,
    ) -> Self::Item<'world, 'state> {
        let value = unsafe { context.world() }
            .get_resource_mut::<T>()
            .unwrap_or_else(|| panic!("required resource `{}` is missing", type_name::<T>()));
        ResMut::new(value)
    }
}

unsafe impl<T: Resource> SystemParam for Option<Res<'static, T>> {
    type State = ();
    type Item<'world, 'state> = Option<Res<'world, T>>;

    fn init(_world: &mut World) -> Self::State {}

    fn register_access(access: &mut SystemAccess) -> Result<(), AccessConflict> {
        access.add_resource_read(type_name::<T>())
    }

    unsafe fn get_param<'world, 'state>(
        _state: &'state mut Self::State,
        context: UnsafeSystemContext<'world>,
    ) -> Self::Item<'world, 'state> {
        unsafe { context.world() }.get_resource::<T>().map(Res::new)
    }
}

unsafe impl<T: Resource> SystemParam for Option<ResMut<'static, T>> {
    type State = ();
    type Item<'world, 'state> = Option<ResMut<'world, T>>;

    fn init(_world: &mut World) -> Self::State {}

    fn register_access(access: &mut SystemAccess) -> Result<(), AccessConflict> {
        access.add_resource_write(type_name::<T>())
    }

    unsafe fn get_param<'world, 'state>(
        _state: &'state mut Self::State,
        context: UnsafeSystemContext<'world>,
    ) -> Self::Item<'world, 'state> {
        unsafe { context.world() }
            .get_resource_mut::<T>()
            .map(ResMut::new)
    }
}

unsafe impl<Q: 'static> SystemParam for Query<'static, Q>
where
    for<'world> Q: QueryTuple<'world>,
{
    type State = ();
    type Item<'world, 'state> = Query<'world, Q>;

    fn init(_world: &mut World) -> Self::State {}

    fn register_access(access: &mut SystemAccess) -> Result<(), AccessConflict> {
        <Q as QueryTuple<'static>>::register_access(access)
    }

    unsafe fn get_param<'world, 'state>(
        _state: &'state mut Self::State,
        context: UnsafeSystemContext<'world>,
    ) -> Self::Item<'world, 'state> {
        Query::new(unsafe { context.world() })
    }
}

unsafe impl<T> SystemParam for Local<'static, T>
where
    T: Default + Send + 'static,
{
    type State = T;
    type Item<'world, 'state> = Local<'state, T>;

    fn init(_world: &mut World) -> Self::State {
        T::default()
    }

    fn register_access(_access: &mut SystemAccess) -> Result<(), AccessConflict> {
        Ok(())
    }

    unsafe fn get_param<'world, 'state>(
        state: &'state mut Self::State,
        _context: UnsafeSystemContext<'world>,
    ) -> Self::Item<'world, 'state> {
        Local { value: state }
    }
}

unsafe impl SystemParam for SystemTicks {
    type State = ();
    type Item<'world, 'state> = SystemTicks;

    fn init(_world: &mut World) -> Self::State {}

    fn register_access(_access: &mut SystemAccess) -> Result<(), AccessConflict> {
        Ok(())
    }

    unsafe fn get_param<'world, 'state>(
        _state: &'state mut Self::State,
        context: UnsafeSystemContext<'world>,
    ) -> Self::Item<'world, 'state> {
        Self {
            last_run_tick: context.last_run_tick,
            this_run_tick: context.this_run_tick,
        }
    }
}

unsafe impl<E: Event> SystemParam for EventReader<'static, 'static, E> {
    type State = ManualEventReader<E>;
    type Item<'world, 'state> = EventReader<'world, 'state, E>;

    fn init(world: &mut World) -> Self::State {
        world
            .get_resource::<Events<E>>()
            .unwrap_or_else(|| {
                panic!(
                    "event `{}` is not registered; call World::add_event first",
                    type_name::<E>()
                )
            })
            .get_reader_current()
    }

    fn register_access(access: &mut SystemAccess) -> Result<(), AccessConflict> {
        access.add_resource_read(type_name::<Events<E>>())
    }

    unsafe fn get_param<'world, 'state>(
        state: &'state mut Self::State,
        context: UnsafeSystemContext<'world>,
    ) -> Self::Item<'world, 'state> {
        let events = unsafe { context.world() }
            .get_resource::<Events<E>>()
            .unwrap_or_else(|| panic!("event `{}` was removed", type_name::<E>()));
        EventReader {
            events,
            reader: state,
        }
    }
}

unsafe impl<E: Event> SystemParam for EventWriter<'static, E> {
    type State = ();
    type Item<'world, 'state> = EventWriter<'world, E>;

    fn init(world: &mut World) -> Self::State {
        assert!(
            world.contains_resource::<Events<E>>(),
            "event `{}` is not registered; call World::add_event first",
            type_name::<E>()
        );
    }

    fn register_access(access: &mut SystemAccess) -> Result<(), AccessConflict> {
        access.add_resource_write(type_name::<Events<E>>())
    }

    unsafe fn get_param<'world, 'state>(
        _state: &'state mut Self::State,
        context: UnsafeSystemContext<'world>,
    ) -> Self::Item<'world, 'state> {
        let events = unsafe { context.world() }
            .get_resource_mut::<Events<E>>()
            .unwrap_or_else(|| panic!("event `{}` was removed", type_name::<E>()));
        EventWriter { events }
    }
}

unsafe impl SystemParam for Globals<'static> {
    type State = ();
    type Item<'world, 'state> = Globals<'world>;

    fn init(_world: &mut World) -> Self::State {}

    fn register_access(access: &mut SystemAccess) -> Result<(), AccessConflict> {
        access.add_globals_read()
    }

    unsafe fn get_param<'world, 'state>(
        _state: &'state mut Self::State,
        context: UnsafeSystemContext<'world>,
    ) -> Self::Item<'world, 'state> {
        Globals {
            value: unsafe { context.globals() },
        }
    }
}

unsafe impl SystemParam for GlobalsMut<'static> {
    type State = ();
    type Item<'world, 'state> = GlobalsMut<'world>;

    fn init(_world: &mut World) -> Self::State {}

    fn register_access(access: &mut SystemAccess) -> Result<(), AccessConflict> {
        access.add_globals_write()
    }

    unsafe fn get_param<'world, 'state>(
        _state: &'state mut Self::State,
        context: UnsafeSystemContext<'world>,
    ) -> Self::Item<'world, 'state> {
        GlobalsMut {
            value: unsafe { context.globals_mut() },
        }
    }
}

unsafe impl SystemParam for &'static mut Commands {
    type State = Commands;
    type Item<'world, 'state> = &'state mut Commands;

    fn init(world: &mut World) -> Self::State {
        Commands::for_world(world)
    }

    fn register_access(access: &mut SystemAccess) -> Result<(), AccessConflict> {
        access.set_deferred();
        Ok(())
    }

    unsafe fn get_param<'world, 'state>(
        state: &'state mut Self::State,
        _context: UnsafeSystemContext<'world>,
    ) -> Self::Item<'world, 'state> {
        state
    }

    fn apply(state: &mut Self::State, context: &mut SystemContext<'_>) {
        state.apply(context);
    }
}

macro_rules! impl_system_param_tuple {
    ($(($param:ident, $state:ident)),+) => {
        unsafe impl<$($param: SystemParam),+> SystemParam for ($($param,)+) {
            type State = ($($param::State,)+);
            type Item<'world, 'state> = ($($param::Item<'world, 'state>,)+);

            fn init(world: &mut World) -> Self::State {
                ($($param::init(world),)+)
            }

            fn register_access(access: &mut SystemAccess) -> Result<(), AccessConflict> {
                $($param::register_access(access)?;)+
                Ok(())
            }

            unsafe fn get_param<'world, 'state>(
                state: &'state mut Self::State,
                context: UnsafeSystemContext<'world>,
            ) -> Self::Item<'world, 'state> {
                let ($($state,)+) = state;
                ($(unsafe { $param::get_param($state, context) },)+)
            }

            fn apply(state: &mut Self::State, context: &mut SystemContext<'_>) {
                let ($($state,)+) = state;
                $($param::apply($state, context);)+
            }
        }
    };
}

impl_system_param_tuple!((P0, s0));
impl_system_param_tuple!((P0, s0), (P1, s1));
impl_system_param_tuple!((P0, s0), (P1, s1), (P2, s2));
impl_system_param_tuple!((P0, s0), (P1, s1), (P2, s2), (P3, s3));
impl_system_param_tuple!((P0, s0), (P1, s1), (P2, s2), (P3, s3), (P4, s4));
impl_system_param_tuple!((P0, s0), (P1, s1), (P2, s2), (P3, s3), (P4, s4), (P5, s5));
impl_system_param_tuple!(
    (P0, s0),
    (P1, s1),
    (P2, s2),
    (P3, s3),
    (P4, s4),
    (P5, s5),
    (P6, s6)
);
impl_system_param_tuple!(
    (P0, s0),
    (P1, s1),
    (P2, s2),
    (P3, s3),
    (P4, s4),
    (P5, s5),
    (P6, s6),
    (P7, s7)
);
impl_system_param_tuple!(
    (P0, s0),
    (P1, s1),
    (P2, s2),
    (P3, s3),
    (P4, s4),
    (P5, s5),
    (P6, s6),
    (P7, s7),
    (P8, s8)
);
impl_system_param_tuple!(
    (P0, s0),
    (P1, s1),
    (P2, s2),
    (P3, s3),
    (P4, s4),
    (P5, s5),
    (P6, s6),
    (P7, s7),
    (P8, s8),
    (P9, s9)
);
impl_system_param_tuple!(
    (P0, s0),
    (P1, s1),
    (P2, s2),
    (P3, s3),
    (P4, s4),
    (P5, s5),
    (P6, s6),
    (P7, s7),
    (P8, s8),
    (P9, s9),
    (P10, s10)
);
impl_system_param_tuple!(
    (P0, s0),
    (P1, s1),
    (P2, s2),
    (P3, s3),
    (P4, s4),
    (P5, s5),
    (P6, s6),
    (P7, s7),
    (P8, s8),
    (P9, s9),
    (P10, s10),
    (P11, s11)
);
impl_system_param_tuple!(
    (P0, s0),
    (P1, s1),
    (P2, s2),
    (P3, s3),
    (P4, s4),
    (P5, s5),
    (P6, s6),
    (P7, s7),
    (P8, s8),
    (P9, s9),
    (P10, s10),
    (P11, s11),
    (P12, s12)
);
impl_system_param_tuple!(
    (P0, s0),
    (P1, s1),
    (P2, s2),
    (P3, s3),
    (P4, s4),
    (P5, s5),
    (P6, s6),
    (P7, s7),
    (P8, s8),
    (P9, s9),
    (P10, s10),
    (P11, s11),
    (P12, s12),
    (P13, s13)
);
impl_system_param_tuple!(
    (P0, s0),
    (P1, s1),
    (P2, s2),
    (P3, s3),
    (P4, s4),
    (P5, s5),
    (P6, s6),
    (P7, s7),
    (P8, s8),
    (P9, s9),
    (P10, s10),
    (P11, s11),
    (P12, s12),
    (P13, s13),
    (P14, s14)
);
impl_system_param_tuple!(
    (P0, s0),
    (P1, s1),
    (P2, s2),
    (P3, s3),
    (P4, s4),
    (P5, s5),
    (P6, s6),
    (P7, s7),
    (P8, s8),
    (P9, s9),
    (P10, s10),
    (P11, s11),
    (P12, s12),
    (P13, s13),
    (P14, s14),
    (P15, s15)
);

pub trait SystemParamFunction<Marker>: Send + 'static {
    type Param: SystemParam;
    type Out: 'static;

    fn run(&mut self, params: <Self::Param as SystemParam>::Item<'_, '_>) -> Self::Out;
}

impl<Func, Out> SystemParamFunction<fn() -> Out> for Func
where
    Func: FnMut() -> Out + Send + 'static,
    Out: 'static,
{
    type Param = ();
    type Out = Out;

    fn run(&mut self, _params: <Self::Param as SystemParam>::Item<'_, '_>) -> Self::Out {
        (self)()
    }
}

macro_rules! impl_system_param_function {
    ($(($param:ident, $value:ident)),+) => {
        impl<Func, Out, $($param),+> SystemParamFunction<fn($($param),+) -> Out> for Func
        where
            $($param: SystemParam,)+
            Func: FnMut($($param),+) -> Out + Send + 'static,
            for<'world, 'state> Func: FnMut($($param::Item<'world, 'state>),+) -> Out,
            Out: 'static,
        {
            type Param = ($($param,)+);
            type Out = Out;

            fn run(&mut self, params: <Self::Param as SystemParam>::Item<'_, '_>) -> Self::Out {
                let ($($value,)+) = params;
                (self)($($value),+)
            }
        }
    };
}

impl_system_param_function!((P0, p0));
impl_system_param_function!((P0, p0), (P1, p1));
impl_system_param_function!((P0, p0), (P1, p1), (P2, p2));
impl_system_param_function!((P0, p0), (P1, p1), (P2, p2), (P3, p3));
impl_system_param_function!((P0, p0), (P1, p1), (P2, p2), (P3, p3), (P4, p4));
impl_system_param_function!((P0, p0), (P1, p1), (P2, p2), (P3, p3), (P4, p4), (P5, p5));
impl_system_param_function!(
    (P0, p0),
    (P1, p1),
    (P2, p2),
    (P3, p3),
    (P4, p4),
    (P5, p5),
    (P6, p6)
);
impl_system_param_function!(
    (P0, p0),
    (P1, p1),
    (P2, p2),
    (P3, p3),
    (P4, p4),
    (P5, p5),
    (P6, p6),
    (P7, p7)
);
impl_system_param_function!(
    (P0, p0),
    (P1, p1),
    (P2, p2),
    (P3, p3),
    (P4, p4),
    (P5, p5),
    (P6, p6),
    (P7, p7),
    (P8, p8)
);
impl_system_param_function!(
    (P0, p0),
    (P1, p1),
    (P2, p2),
    (P3, p3),
    (P4, p4),
    (P5, p5),
    (P6, p6),
    (P7, p7),
    (P8, p8),
    (P9, p9)
);
impl_system_param_function!(
    (P0, p0),
    (P1, p1),
    (P2, p2),
    (P3, p3),
    (P4, p4),
    (P5, p5),
    (P6, p6),
    (P7, p7),
    (P8, p8),
    (P9, p9),
    (P10, p10)
);
impl_system_param_function!(
    (P0, p0),
    (P1, p1),
    (P2, p2),
    (P3, p3),
    (P4, p4),
    (P5, p5),
    (P6, p6),
    (P7, p7),
    (P8, p8),
    (P9, p9),
    (P10, p10),
    (P11, p11)
);
impl_system_param_function!(
    (P0, p0),
    (P1, p1),
    (P2, p2),
    (P3, p3),
    (P4, p4),
    (P5, p5),
    (P6, p6),
    (P7, p7),
    (P8, p8),
    (P9, p9),
    (P10, p10),
    (P11, p11),
    (P12, p12)
);
impl_system_param_function!(
    (P0, p0),
    (P1, p1),
    (P2, p2),
    (P3, p3),
    (P4, p4),
    (P5, p5),
    (P6, p6),
    (P7, p7),
    (P8, p8),
    (P9, p9),
    (P10, p10),
    (P11, p11),
    (P12, p12),
    (P13, p13)
);
impl_system_param_function!(
    (P0, p0),
    (P1, p1),
    (P2, p2),
    (P3, p3),
    (P4, p4),
    (P5, p5),
    (P6, p6),
    (P7, p7),
    (P8, p8),
    (P9, p9),
    (P10, p10),
    (P11, p11),
    (P12, p12),
    (P13, p13),
    (P14, p14)
);
impl_system_param_function!(
    (P0, p0),
    (P1, p1),
    (P2, p2),
    (P3, p3),
    (P4, p4),
    (P5, p5),
    (P6, p6),
    (P7, p7),
    (P8, p8),
    (P9, p9),
    (P10, p10),
    (P11, p11),
    (P12, p12),
    (P13, p13),
    (P14, p14),
    (P15, p15)
);

pub trait IntoSystem<Marker>: Sized {
    type System: RunnableSystem;

    fn into_system(self) -> Self::System;
}

pub struct ParamSystemMarker<Marker>(PhantomData<fn() -> Marker>);

impl<Func, Marker> IntoSystem<ParamSystemMarker<Marker>> for Func
where
    Func: SystemParamFunction<Marker>,
    Marker: 'static,
{
    type System = FunctionSystem<Func, Marker>;

    fn into_system(self) -> Self::System {
        FunctionSystem {
            function: self,
            state: None,
            access: SystemAccess::default(),
            marker: PhantomData,
        }
    }
}

impl<Func> LegacyFunctionSystem<Func>
where
    Func: for<'world> FnMut(&mut SystemContext<'world>, &mut Commands) + Send + 'static,
{
    pub(crate) fn new(function: Func) -> Self {
        let mut access = SystemAccess::default();
        access
            .set_context_exclusive()
            .expect("a new legacy system has no previous access");
        access.set_deferred();
        Self {
            function,
            commands: Commands::new(),
            access,
        }
    }
}

pub trait RunnableSystem: Send + 'static {
    fn initialize(&mut self, world: &mut World);
    fn access(&self) -> &SystemAccess;
    fn run(&mut self, context: &mut SystemContext<'_>);
}

pub type System = Box<dyn RunnableSystem>;

pub struct LegacyFunctionSystem<Func> {
    function: Func,
    commands: Commands,
    access: SystemAccess,
}

impl<Func> RunnableSystem for LegacyFunctionSystem<Func>
where
    Func: for<'world> FnMut(&mut SystemContext<'world>, &mut Commands) + Send + 'static,
{
    fn initialize(&mut self, world: &mut World) {
        self.commands.attach_world(world);
    }

    fn access(&self) -> &SystemAccess {
        &self.access
    }

    fn run(&mut self, context: &mut SystemContext<'_>) {
        (self.function)(context, &mut self.commands);
        self.commands.apply(context);
    }
}

pub struct FunctionSystem<Func, Marker>
where
    Func: SystemParamFunction<Marker>,
{
    function: Func,
    state: Option<<Func::Param as SystemParam>::State>,
    access: SystemAccess,
    marker: PhantomData<fn() -> Marker>,
}

impl<Func, Marker> RunnableSystem for FunctionSystem<Func, Marker>
where
    Func: SystemParamFunction<Marker>,
    Marker: 'static,
{
    fn initialize(&mut self, world: &mut World) {
        if self.state.is_some() {
            return;
        }

        let mut access = SystemAccess::default();
        Func::Param::register_access(&mut access)
            .unwrap_or_else(|error| panic!("invalid system parameters: {error}"));
        self.access = access;
        self.state = Some(Func::Param::init(world));
    }

    fn access(&self) -> &SystemAccess {
        &self.access
    }

    fn run(&mut self, context: &mut SystemContext<'_>) {
        self.initialize(context.world);
        let state = self.state.as_mut().expect("system must be initialized");

        {
            let unsafe_context = UnsafeSystemContext::new(context);
            let params = unsafe { Func::Param::get_param(state, unsafe_context) };
            let _ = self.function.run(params);
        }

        Func::Param::apply(state, context);
    }
}

pub struct HotSystem<S>
where
    S: RunnableSystem,
{
    name: &'static str,
    system: S,
    current_ptr: subsecond::HotFnPtr,
}

impl<S> HotSystem<S>
where
    S: RunnableSystem,
{
    pub fn new(name: &'static str, system: S) -> Self {
        Self {
            name,
            system,
            current_ptr: subsecond::HotFn::current(run_system::<S>).ptr_address(),
        }
    }
}

impl<S> RunnableSystem for HotSystem<S>
where
    S: RunnableSystem,
{
    fn initialize(&mut self, world: &mut World) {
        self.system.initialize(world);
    }

    fn access(&self) -> &SystemAccess {
        self.system.access()
    }

    fn run(&mut self, context: &mut SystemContext<'_>) {
        let mut hot = subsecond::HotFn::current(run_system::<S>);
        let current_ptr = hot.ptr_address();
        if current_ptr != self.current_ptr {
            log::debug!("ECS system hotpatch pointer refreshed: {}", self.name);
            self.current_ptr = current_ptr;
        }

        // SAFETY: current_ptr always comes from the same run_system::<S> HotFn signature.
        unsafe {
            hot.try_call_with_ptr(self.current_ptr, (&mut self.system, context))
                .expect("failed to call hotpatched ECS system; restart with a full rebuild");
        }
    }
}

fn run_system<S>(system: &mut S, context: &mut SystemContext<'_>)
where
    S: RunnableSystem,
{
    system.run(context);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::{component::Component, event::Events};

    #[derive(Default)]
    struct Counter(u32);

    struct TestComponent(u32);

    fn run_world_params<Func, Marker>(
        function: &mut Func,
        state: &mut Option<<Func::Param as SystemParam>::State>,
        world: &mut World,
    ) where
        Func: SystemParamFunction<Marker>,
    {
        let state = state.get_or_insert_with(|| Func::Param::init(world));
        let mut access = SystemAccess::default();
        Func::Param::register_access(&mut access).unwrap();
        let context = unsafe { UnsafeSystemContext::from_world(world) };
        let params = unsafe { Func::Param::get_param(state, context) };
        let _ = function.run(params);
    }

    #[test]
    fn resources_and_queries_are_automatically_fetched() {
        fn system(mut counter: ResMut<Counter>, mut query: Query<(&mut TestComponent,)>) {
            query.for_each(|_, (component,)| {
                counter.0 += component.0;
                component.0 += 1;
            });
        }

        fn assert_component<T: Component>() {}
        assert_component::<TestComponent>();

        let mut world = World::new();
        world.insert_opaque_resource(Counter::default());
        let entity = world.spawn();
        world.insert_opaque(entity, TestComponent(3));
        let mut state = None;

        run_world_params(&mut system, &mut state, &mut world);

        assert_eq!(world.get_resource::<Counter>().unwrap().0, 3);
        assert_eq!(world.get::<TestComponent>(entity).unwrap().0, 4);
    }

    #[test]
    fn local_state_persists_between_runs() {
        fn system(mut local: Local<u32>, mut counter: ResMut<Counter>) {
            *local += 1;
            counter.0 = *local;
        }

        let mut world = World::new();
        world.insert_opaque_resource(Counter::default());
        let mut state = None;

        run_world_params(&mut system, &mut state, &mut world);
        run_world_params(&mut system, &mut state, &mut world);

        assert_eq!(world.get_resource::<Counter>().unwrap().0, 2);
    }

    #[derive(Debug, PartialEq, Eq)]
    struct Ping(u32);

    #[test]
    fn event_readers_keep_independent_system_state() {
        fn writer(mut events: EventWriter<Ping>) {
            events.send(Ping(7));
        }
        fn reader(mut events: EventReader<Ping>, mut counter: ResMut<Counter>) {
            counter.0 += events.read().map(|event| event.0).sum::<u32>();
        }

        let mut world = World::new();
        world.add_event::<Ping>();
        world.insert_opaque_resource(Counter::default());
        let mut writer_state = None;
        let mut reader_state = None;

        run_world_params(&mut reader, &mut reader_state, &mut world);
        run_world_params(&mut writer, &mut writer_state, &mut world);
        run_world_params(&mut reader, &mut reader_state, &mut world);
        run_world_params(&mut reader, &mut reader_state, &mut world);

        assert_eq!(world.get_resource::<Counter>().unwrap().0, 7);
        assert_eq!(world.get_resource::<Events<Ping>>().unwrap().len(), 1);
    }

    #[test]
    fn conflicting_query_parameters_fail_during_access_registration() {
        type Invalid = (
            Query<'static, (&'static TestComponent,)>,
            Query<'static, (&'static mut TestComponent,)>,
        );
        let mut access = SystemAccess::default();
        assert!(Invalid::register_access(&mut access).is_err());
    }
}

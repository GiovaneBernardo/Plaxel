use crate::{
    ecs::{change::ChangeTick, commands::Commands, world::World},
    global_resources::GlobalResources,
};
pub struct SystemContext<'a> {
    pub world: &'a mut World,
    pub globals: &'a mut GlobalResources,
    pub last_run_tick: ChangeTick,
    pub this_run_tick: ChangeTick,
}

pub trait RunnableSystem {
    fn run(&mut self, ctx: &mut SystemContext, commands: &mut Commands);
}

pub type System = Box<dyn RunnableSystem>;

pub struct StaticSystem<F>
where
    F: FnMut(&mut SystemContext, &mut Commands) + 'static,
{
    system: F,
}

impl<F> StaticSystem<F>
where
    F: FnMut(&mut SystemContext, &mut Commands) + 'static,
{
    pub fn new(system: F) -> Self {
        Self { system }
    }
}

impl<F> RunnableSystem for StaticSystem<F>
where
    F: FnMut(&mut SystemContext, &mut Commands) + 'static,
{
    fn run(&mut self, ctx: &mut SystemContext, commands: &mut Commands) {
        (self.system)(ctx, commands);
    }
}

pub struct HotSystem<F>
where
    F: FnMut(&mut SystemContext, &mut Commands) + 'static,
{
    name: &'static str,
    system: F,
    current_ptr: subsecond::HotFnPtr,
}

impl<F> HotSystem<F>
where
    F: FnMut(&mut SystemContext, &mut Commands) + 'static,
{
    pub fn new(name: &'static str, system: F) -> Self {
        Self {
            name,
            system,
            current_ptr: subsecond::HotFn::current(run_system::<F>).ptr_address(),
        }
    }
}

impl<F> RunnableSystem for HotSystem<F>
where
    F: FnMut(&mut SystemContext, &mut Commands) + 'static,
{
    fn run(&mut self, ctx: &mut SystemContext, commands: &mut Commands) {
        let mut hot = subsecond::HotFn::current(run_system::<F>);
        let current_ptr = hot.ptr_address();
        if current_ptr != self.current_ptr {
            log::debug!("ECS system hotpatch pointer refreshed: {}", self.name);
            self.current_ptr = current_ptr;
        }

        // SAFETY: current_ptr always comes from the same run_system::<F> HotFn signature.
        unsafe {
            hot.try_call_with_ptr(self.current_ptr, (&mut self.system, ctx, commands))
                .expect("failed to call hotpatched ECS system; restart with a full rebuild");
        }
    }
}

fn run_system<F>(system: &mut F, ctx: &mut SystemContext, commands: &mut Commands)
where
    F: FnMut(&mut SystemContext, &mut Commands) + 'static,
{
    system(ctx, commands);
}

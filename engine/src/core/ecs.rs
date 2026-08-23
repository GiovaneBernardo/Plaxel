pub mod access;
pub mod change;
pub mod commands;
pub mod component;
pub mod entity;
pub mod event;
pub mod plugin;
pub mod query;
pub mod resource;
pub mod scene;
pub mod schedule;
pub mod storage;
pub mod system;
pub mod world;

pub mod prelude {
    pub use super::{
        access::SystemAccess,
        commands::{Bundle, Commands, EntityCommands},
        entity::Entity,
        event::{
            Event, EventIterator, EventReader, EventWriter, Events, ManualEventReader,
            event_update_system,
        },
        plugin::Plugin,
        query::Query,
        resource::{Res, ResMut, Resource},
        schedule::CoreSchedule,
        schedule::Schedule,
        system::{
            Globals, GlobalsMut, IntoSystem, Local, SystemParam, SystemParamFunction, SystemTicks,
        },
    };
}

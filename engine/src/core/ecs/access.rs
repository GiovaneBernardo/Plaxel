use std::{collections::HashSet, fmt};

use crate::ecs::{component::Component, resource::Resource};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessConflict {
    message: String,
}

impl AccessConflict {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for AccessConflict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AccessConflict {}

/// The component, resource, and engine-global data a system may access.
///
/// This is collected when a system is initialized. The current scheduler is
/// sequential, but keeping the access declaration on every system makes
/// conflict validation possible now and parallel scheduling possible later.
#[derive(Debug, Clone, Default)]
pub struct SystemAccess {
    component_reads: HashSet<&'static str>,
    component_writes: HashSet<&'static str>,
    resource_reads: HashSet<&'static str>,
    resource_writes: HashSet<&'static str>,
    world_exclusive: bool,
    globals_read: bool,
    globals_write: bool,
    globals_exclusive: bool,
    deferred: bool,
}

impl SystemAccess {
    /// Declares shared access to component storage `T`.
    pub fn read_component<T: Component>(&mut self) -> Result<(), AccessConflict> {
        self.add_component_read(std::any::type_name::<T>())
    }

    /// Declares exclusive access to component storage `T`.
    pub fn write_component<T: Component>(&mut self) -> Result<(), AccessConflict> {
        self.add_component_write(std::any::type_name::<T>())
    }

    /// Declares shared access to resource `T`.
    pub fn read_resource<T: Resource>(&mut self) -> Result<(), AccessConflict> {
        self.add_resource_read(std::any::type_name::<T>())
    }

    /// Declares exclusive access to resource `T`.
    pub fn write_resource<T: Resource>(&mut self) -> Result<(), AccessConflict> {
        self.add_resource_write(std::any::type_name::<T>())
    }

    pub(crate) fn add_component_read(&mut self, name: &'static str) -> Result<(), AccessConflict> {
        self.ensure_world_is_not_exclusive(name)?;
        if self.component_writes.contains(name) {
            return Err(AccessConflict::new(format!(
                "component `{name}` is already mutably accessed"
            )));
        }
        self.component_reads.insert(name);
        Ok(())
    }

    pub(crate) fn add_component_write(&mut self, name: &'static str) -> Result<(), AccessConflict> {
        self.ensure_world_is_not_exclusive(name)?;
        if self.component_reads.contains(name) || self.component_writes.contains(name) {
            return Err(AccessConflict::new(format!(
                "component `{name}` is already accessed"
            )));
        }
        self.component_writes.insert(name);
        Ok(())
    }

    pub(crate) fn add_resource_read(&mut self, name: &'static str) -> Result<(), AccessConflict> {
        self.ensure_world_is_not_exclusive(name)?;
        if self.resource_writes.contains(name) {
            return Err(AccessConflict::new(format!(
                "resource `{name}` is already mutably accessed"
            )));
        }
        self.resource_reads.insert(name);
        Ok(())
    }

    pub(crate) fn add_resource_write(&mut self, name: &'static str) -> Result<(), AccessConflict> {
        self.ensure_world_is_not_exclusive(name)?;
        if self.resource_reads.contains(name) || self.resource_writes.contains(name) {
            return Err(AccessConflict::new(format!(
                "resource `{name}` is already accessed"
            )));
        }
        self.resource_writes.insert(name);
        Ok(())
    }

    pub(crate) fn add_globals_read(&mut self) -> Result<(), AccessConflict> {
        if self.globals_write || self.globals_exclusive {
            return Err(AccessConflict::new(
                "GlobalResources is already mutably accessed",
            ));
        }
        self.globals_read = true;
        Ok(())
    }

    pub(crate) fn add_globals_write(&mut self) -> Result<(), AccessConflict> {
        if self.globals_read || self.globals_write || self.globals_exclusive {
            return Err(AccessConflict::new("GlobalResources is already accessed"));
        }
        self.globals_write = true;
        Ok(())
    }

    pub(crate) fn set_context_exclusive(&mut self) -> Result<(), AccessConflict> {
        if self.has_world_access() || self.has_globals_access() {
            return Err(AccessConflict::new(
                "an exclusive SystemContext cannot be combined with other world or global parameters",
            ));
        }
        self.world_exclusive = true;
        self.globals_exclusive = true;
        Ok(())
    }

    pub(crate) fn set_deferred(&mut self) {
        self.deferred = true;
    }

    fn ensure_world_is_not_exclusive(&self, name: &'static str) -> Result<(), AccessConflict> {
        if self.world_exclusive {
            return Err(AccessConflict::new(format!(
                "`{name}` cannot be accessed alongside an exclusive SystemContext"
            )));
        }
        Ok(())
    }

    fn has_world_access(&self) -> bool {
        self.world_exclusive
            || !self.component_reads.is_empty()
            || !self.component_writes.is_empty()
            || !self.resource_reads.is_empty()
            || !self.resource_writes.is_empty()
    }

    fn has_globals_access(&self) -> bool {
        self.globals_read || self.globals_write || self.globals_exclusive
    }

    pub fn is_exclusive(&self) -> bool {
        self.world_exclusive || self.globals_exclusive
    }

    pub fn has_deferred(&self) -> bool {
        self.deferred
    }

    pub fn conflicts_with(&self, other: &Self) -> bool {
        let world_exclusive_conflict = (self.world_exclusive && other.has_world_access())
            || (other.world_exclusive && self.has_world_access());
        let globals_exclusive_conflict = (self.globals_exclusive && other.has_globals_access())
            || (other.globals_exclusive && self.has_globals_access());

        world_exclusive_conflict
            || globals_exclusive_conflict
            || intersects(&self.component_writes, &other.component_reads)
            || intersects(&self.component_writes, &other.component_writes)
            || intersects(&other.component_writes, &self.component_reads)
            || intersects(&self.resource_writes, &other.resource_reads)
            || intersects(&self.resource_writes, &other.resource_writes)
            || intersects(&other.resource_writes, &self.resource_reads)
            || (self.globals_write && (other.globals_read || other.globals_write))
            || (other.globals_write && (self.globals_read || self.globals_write))
    }
}

fn intersects(left: &HashSet<&'static str>, right: &HashSet<&'static str>) -> bool {
    left.iter().any(|item| right.contains(item))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_are_compatible_but_writes_conflict() {
        let mut first = SystemAccess::default();
        first.add_component_read("Transform").unwrap();
        let mut second = SystemAccess::default();
        second.add_component_read("Transform").unwrap();
        assert!(!first.conflicts_with(&second));

        let mut writer = SystemAccess::default();
        writer.add_component_write("Transform").unwrap();
        assert!(first.conflicts_with(&writer));
    }

    #[test]
    fn conflicting_parameters_in_one_system_are_rejected() {
        let mut access = SystemAccess::default();
        access.add_resource_read("Time").unwrap();
        assert!(access.add_resource_write("Time").is_err());
    }
}

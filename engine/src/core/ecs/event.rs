use std::{
    cell::{Ref, RefMut},
    iter::Chain,
    marker::PhantomData,
    slice::Iter,
};

use crate::ecs::resource::ResMut;

pub trait Event: Send + Sync + 'static {}

impl<T: Send + Sync + 'static> Event for T {}

#[derive(Debug)]
struct EventInstance<E> {
    id: u64,
    event: E,
}

#[derive(Debug)]
struct EventSequence<E> {
    start_event_count: u64,
    events: Vec<EventInstance<E>>,
}

impl<E> Default for EventSequence<E> {
    fn default() -> Self {
        Self {
            start_event_count: 0,
            events: Vec::new(),
        }
    }
}

/// Double-buffered event storage. Events remain readable for two calls to
/// [`Events::update`], allowing readers in different schedules to keep their
/// own cursors without consuming events globally.
#[derive(Debug)]
pub struct Events<E: Event> {
    oldest: EventSequence<E>,
    current: EventSequence<E>,
    event_count: u64,
}

impl<E: Event> Default for Events<E> {
    fn default() -> Self {
        Self {
            oldest: EventSequence::default(),
            current: EventSequence::default(),
            event_count: 0,
        }
    }
}

impl<E: Event> Events<E> {
    pub fn send(&mut self, event: E) -> u64 {
        let id = self.event_count;
        self.event_count = self.event_count.wrapping_add(1);
        self.current.events.push(EventInstance { id, event });
        id
    }

    pub fn send_batch(&mut self, events: impl IntoIterator<Item = E>) {
        for event in events {
            self.send(event);
        }
    }

    pub fn update(&mut self) {
        self.oldest.events.clear();
        self.oldest.start_event_count = self.event_count;
        std::mem::swap(&mut self.oldest, &mut self.current);
    }

    pub fn clear(&mut self) {
        self.oldest.events.clear();
        self.current.events.clear();
        self.oldest.start_event_count = self.event_count;
        self.current.start_event_count = self.event_count;
    }

    pub fn len(&self) -> usize {
        self.oldest.events.len() + self.current.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get_reader(&self) -> ManualEventReader<E> {
        ManualEventReader {
            last_event_count: self.oldest.start_event_count,
            marker: PhantomData,
        }
    }

    pub fn get_reader_current(&self) -> ManualEventReader<E> {
        ManualEventReader {
            last_event_count: self.event_count,
            marker: PhantomData,
        }
    }
}

#[derive(Debug)]
pub struct ManualEventReader<E: Event> {
    last_event_count: u64,
    marker: PhantomData<fn() -> E>,
}

impl<E: Event> Default for ManualEventReader<E> {
    fn default() -> Self {
        Self {
            last_event_count: 0,
            marker: PhantomData,
        }
    }
}

impl<E: Event> ManualEventReader<E> {
    pub fn read<'events, 'reader>(
        &'reader mut self,
        events: &'events Events<E>,
    ) -> EventIterator<'events, 'reader, E> {
        let first_available = events.oldest.start_event_count;
        let unread_from = self.last_event_count.max(first_available);

        EventIterator {
            reader: self,
            events: events
                .oldest
                .events
                .iter()
                .chain(events.current.events.iter()),
            unread_from,
            event_count: events.event_count,
        }
    }

    pub fn missed_events(&self, events: &Events<E>) -> u64 {
        events
            .oldest
            .start_event_count
            .saturating_sub(self.last_event_count)
    }
}

/// An event iterator that advances its reader cursor as items are consumed.
/// Dropping it early leaves the remaining events unread for the next call.
pub struct EventIterator<'events, 'reader, E: Event> {
    reader: &'reader mut ManualEventReader<E>,
    events: Chain<Iter<'events, EventInstance<E>>, Iter<'events, EventInstance<E>>>,
    unread_from: u64,
    event_count: u64,
}

impl<'events, E: Event> Iterator for EventIterator<'events, '_, E> {
    type Item = &'events E;

    fn next(&mut self) -> Option<Self::Item> {
        for event in self.events.by_ref() {
            if event.id >= self.unread_from {
                self.reader.last_event_count = event.id.wrapping_add(1);
                return Some(&event.event);
            }
        }

        self.reader.last_event_count = self.event_count;
        None
    }
}

pub struct EventReader<'w, 's, E: Event> {
    pub(crate) events: Ref<'w, Events<E>>,
    pub(crate) reader: &'s mut ManualEventReader<E>,
}

impl<E: Event> EventReader<'_, '_, E> {
    pub fn read(&mut self) -> EventIterator<'_, '_, E> {
        self.reader.read(&self.events)
    }

    pub fn missed_events(&self) -> u64 {
        self.reader.missed_events(&self.events)
    }
}

pub struct EventWriter<'w, E: Event> {
    pub(crate) events: RefMut<'w, Events<E>>,
}

impl<E: Event> EventWriter<'_, E> {
    pub fn send(&mut self, event: E) -> u64 {
        self.events.send(event)
    }

    pub fn send_batch(&mut self, events: impl IntoIterator<Item = E>) {
        self.events.send_batch(events);
    }
}

pub fn event_update_system<E: Event>(mut events: ResMut<Events<E>>) {
    events.update();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    struct TestEvent(u32);

    #[test]
    fn readers_have_independent_cursors() {
        let mut events = Events::default();
        let mut first = events.get_reader_current();
        let mut second = events.get_reader_current();
        events.send(TestEvent(1));

        assert_eq!(first.read(&events).collect::<Vec<_>>(), vec![&TestEvent(1)]);
        assert_eq!(first.read(&events).count(), 0);
        assert_eq!(
            second.read(&events).collect::<Vec<_>>(),
            vec![&TestEvent(1)]
        );
    }

    #[test]
    fn events_expire_after_two_updates() {
        let mut events = Events::default();
        let mut reader = events.get_reader_current();
        events.send(TestEvent(1));
        events.update();
        events.update();

        assert_eq!(reader.missed_events(&events), 1);
        assert_eq!(reader.read(&events).count(), 0);
    }

    #[test]
    fn dropping_an_iterator_early_does_not_skip_events() {
        let mut events = Events::default();
        let mut reader = events.get_reader_current();
        events.send(TestEvent(1));
        events.send(TestEvent(2));

        assert_eq!(reader.read(&events).next(), Some(&TestEvent(1)));
        assert_eq!(reader.read(&events).next(), Some(&TestEvent(2)));
        assert_eq!(reader.read(&events).next(), None);
    }
}

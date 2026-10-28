//! C#'s `event Action<T>` for a single-threaded shell: handlers connect, disconnect and are
//! emitted to in connection order. Emission walks a snapshot, so a handler may connect or
//! disconnect (itself or another) while the event is being raised — the same guarantee a C#
//! multicast delegate gives, and the one a GTK signal gives.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

/// The handle [`Event::connect`] returns, for [`Event::disconnect`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HandlerId(u64);

/// A list of handlers taking `&A`. Use `Event<()>` for `event Action`.
pub struct Event<A: ?Sized> {
    next: Cell<u64>,
    handlers: RefCell<Vec<(HandlerId, Rc<dyn Fn(&A)>)>>,
}

impl<A: ?Sized> Default for Event<A> {
    fn default() -> Self {
        Event { next: Cell::new(0), handlers: RefCell::new(Vec::new()) }
    }
}

impl<A: ?Sized> fmt::Debug for Event<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Event").field("handlers", &self.handlers.borrow().len()).finish()
    }
}

impl<A: ?Sized> Event<A> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a handler; it runs after every handler already connected.
    pub fn connect(&self, handler: impl Fn(&A) + 'static) -> HandlerId {
        let id = HandlerId(self.next.get());
        self.next.set(id.0 + 1);
        self.handlers.borrow_mut().push((id, Rc::new(handler)));
        id
    }

    /// Removes a handler; unknown ids are ignored.
    pub fn disconnect(&self, id: HandlerId) {
        self.handlers.borrow_mut().retain(|(h, _)| *h != id);
    }

    /// Raises the event. Handlers connected during the emission are not called this time; a
    /// handler disconnected during it is skipped if it has not run yet.
    pub fn emit(&self, argument: &A) {
        let snapshot: Vec<(HandlerId, Rc<dyn Fn(&A)>)> = self.handlers.borrow().clone();
        for (id, handler) in snapshot {
            if self.handlers.borrow().iter().any(|(h, _)| *h == id) {
                handler(argument);
            }
        }
    }

    pub fn handler_count(&self) -> usize {
        self.handlers.borrow().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handlers_run_in_order_and_can_disconnect_during_emission() {
        let event: Rc<Event<i32>> = Rc::new(Event::new());
        let log = Rc::new(RefCell::new(Vec::new()));
        let second = Rc::new(Cell::new(None));
        {
            let (log, event2, second) = (log.clone(), event.clone(), second.clone());
            event.connect(move |v| {
                log.borrow_mut().push(("first", *v));
                if let Some(id) = second.get() {
                    event2.disconnect(id);
                }
            });
        }
        {
            let log = log.clone();
            second.set(Some(event.connect(move |v| log.borrow_mut().push(("second", *v)))));
        }
        event.emit(&1);
        assert_eq!(*log.borrow(), vec![("first", 1)]);
        assert_eq!(event.handler_count(), 1);
    }

    #[test]
    fn a_handler_connected_while_emitting_waits_for_the_next_emission() {
        let event: Rc<Event<()>> = Rc::new(Event::new());
        let calls = Rc::new(Cell::new(0));
        {
            let (event2, calls) = (event.clone(), calls.clone());
            event.connect(move |_| {
                let calls = calls.clone();
                event2.connect(move |_| calls.set(calls.get() + 1));
            });
        }
        event.emit(&());
        assert_eq!(calls.get(), 0);
        event.emit(&());
        assert_eq!(calls.get(), 1);
    }
}

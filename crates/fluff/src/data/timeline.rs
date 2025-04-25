use std::cell::Cell;
use std::rc::Rc;
use kyute::event::EmitterHandle;
use kyute::EventSource;

/// Events emitted by `Timeline`s.
#[derive(Debug, Clone, Copy)]
pub enum TimelineEvent {
    /// The current time on the timeline has changed.
    TimeChanged {
        /// Whether the change was made by the user or programmatically.
        by_user: bool,
        /// The new time in seconds.
        time: f64,
    },
    EndTimeChanged {
        /// The new end time in seconds.
        time: f64,
        
    }
}

/// The state of a timeline: its current time and temporal extent.
pub struct Timeline {
    emitter: EmitterHandle,
    /// The current position of the playhead in seconds.
    pub current_time: Cell<f64>,
    /// End time.
    pub end_time: Cell<f64>,
}

impl Timeline {
    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            emitter: EmitterHandle::new(),
            current_time: Cell::new(0.0),
            end_time: Cell::new(0.0),
        })
    }

    /// Sets the current time and signals the change.
    pub fn set_current_time(&self, time: f64) {
        self.current_time.set(time);
        self.emitter.emit(TimelineEvent::TimeChanged {
            by_user: false,
            time,
        });
    }

    pub fn set_current_time_by_user(&self, time: f64) {
        self.current_time.set(time);
        self.emitter.emit(TimelineEvent::TimeChanged {
            by_user: true,
            time,
        });
    }
    
    pub fn set_end_time(&self, time: f64) {
        self.end_time.set(time);
        self.emitter.emit(TimelineEvent::EndTimeChanged { time });
    }
}

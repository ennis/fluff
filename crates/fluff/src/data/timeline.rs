use kyute::model::Model;

#[derive(Debug, Clone, Copy)]
pub enum TimelineEvent {
    TimeChanged(f64),
}


/// Information about the current time.
pub struct Timeline {
    current_time: f64,
}


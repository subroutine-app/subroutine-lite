
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Action, PipelineContext, SchedulePoint, schedule::find_free_slot};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchPlacement {
    pub action: Action,
    pub displaced: Vec<Action>,
    pub cursor: DateTime<Utc>,
}

impl BatchPlacement {
    pub fn moved(&self) -> impl Iterator<Item = &Action> {
        std::iter::once(&self.action).chain(self.displaced.iter())
    }
}

impl PipelineContext<'_> {
    pub fn batch_start(&self) -> DateTime<Utc> {
        self.quantized_now()
    }

    pub fn place_in_batch(&self, cursor: DateTime<Utc>, action: Action) -> BatchPlacement {
        let from = match action.start {
            Some(SchedulePoint::DateTime(named)) => named,
            _ => cursor,
        };
        let duration = self.effective_duration(&action);

        let anchors = self.build_anchors(Some(self.quantized_now()));
        let start = find_free_slot(from, duration, &anchors, self.config.granularity);

        let mut placed = action;
        placed.set_queued(true);
        placed.set_start(Some(SchedulePoint::DateTime(start)));

        BatchPlacement {
            displaced: self.push_actions_forward(start, duration),
            action: placed,
            cursor: self.quantize_ceil(start + duration),
        }
    }
}

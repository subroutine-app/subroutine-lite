use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    Action, PipelineContext, SchedulePoint,
    schedule::{duration_end, find_free_slot, scheduled_time},
};

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
    pub fn batch_start(&self) -> Result<DateTime<Utc>, &'static str> {
        self.quantized_now()
    }

    pub fn place_in_batch(
        &self,
        cursor: DateTime<Utc>,
        action: Action,
    ) -> Result<BatchPlacement, &'static str> {
        let actions: Vec<_> = self
            .actions
            .iter()
            .filter(|existing| existing.id != action.id)
            .cloned()
            .collect();
        let context = PipelineContext {
            actions: &actions,
            ..*self
        };
        let from = match action.start {
            Some(SchedulePoint::DateTime(named)) => named,
            _ => cursor,
        };
        let duration = context.effective_duration(&action);

        let anchors = context.build_anchors(Some(from))?;
        let mut start = find_free_slot(from, duration, &anchors, context.config.granularity)?;
        let mut earlier: Vec<_> = context
            .actions
            .iter()
            .filter(|action| !action.pinned && !action.is_completed())
            .filter_map(|action| scheduled_time(action).map(|start| (start, action)))
            .collect();
        earlier.sort_by_key(|(start, action)| (*start, action.id));
        for (old_start, action) in earlier {
            if old_start >= start {
                break;
            }
            let end = duration_end(old_start, context.effective_duration(action))?;
            if end > start {
                start = find_free_slot(end, duration, &anchors, context.config.granularity)?;
            }
        }

        let mut placed = action;
        placed.set_queued(true);
        placed.set_start(Some(SchedulePoint::DateTime(start)));

        Ok(BatchPlacement {
            displaced: context.push_actions_forward(start, duration)?,
            action: placed,
            cursor: context.quantize_ceil(duration_end(start, duration)?)?,
        })
    }
}

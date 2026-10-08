use std::collections::{HashMap, HashSet};

use gpui::{App, Pixels, Window, px};
use uuid::Uuid;

use crate::components::{ItemCardDetails, ItemCardStates};

#[derive(Clone)]
struct CardRow {
    details: ItemCardDetails,
    height: Pixels,
}

#[derive(Clone, Default)]
pub(crate) struct DynamicListCardStates {
    states: ItemCardStates,
    rows: HashMap<Uuid, CardRow>,
    generation: u64,
}

impl DynamicListCardStates {
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn set_generation(&mut self, generation: u64) -> bool {
        if self.generation == generation {
            return false;
        }
        *self = Self {
            generation,
            ..Self::default()
        };
        true
    }

    pub(crate) fn retain(&mut self, ids: impl IntoIterator<Item = Uuid>) {
        let ids: HashSet<_> = ids.into_iter().collect();
        self.states.retain(ids.iter().copied());
        self.rows.retain(|id, _| ids.contains(id));
        for id in ids {
            self.rows.entry(id).or_insert_with(|| CardRow {
                details: self.states.get(id),
                height: px(0.),
            });
        }
    }

    pub(crate) fn get(&self, id: Uuid) -> ItemCardDetails {
        self.rows
            .get(&id)
            .map(|row| row.details.clone())
            .unwrap_or_default()
    }

    pub(crate) fn height(&self, id: Uuid, collapsed: Pixels) -> Pixels {
        self.rows
            .get(&id)
            .map_or(collapsed, |row| row.height.max(collapsed))
    }

    pub(crate) fn pause(&self) {
        self.states.pause();
    }

    pub(crate) fn animate(&mut self, collapsed: Pixels, window: &mut Window, cx: &mut App) {
        self.states.animate(window, cx);
        for (id, row) in &mut self.rows {
            row.height = self.states.height(*id, collapsed);
        }
    }
}

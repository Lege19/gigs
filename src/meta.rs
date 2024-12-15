use std::{
    cmp::Ordering,
    num::NonZero,
    ops::{Add, AddAssign},
};

use bevy_ecs::{
    component::Component,
    query::Added,
    system::{Commands, Query},
};
use bevy_render::{sync_world::RenderEntity, Extract};

/// The priority level of a graphics job.
///
/// Jobs with [`Priority::NonCritical`] will be executed in order of priority,
/// from highest to lowest.
///
/// All jobs with [`Priority::Critical`] will be executed *during the current frame*.
/// The renderer will wait for all its dependencies to finish and block on pipeline compilation,
/// which may cause stutter. **USE THIS VARIANT SPARINGLY**
///
/// Jobs propagate their priority to their dependencies additively, so jobs with many
/// dependents are prioritized.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum Priority {
    Critical,
    NonCritical(NonZero<u32>),
}

impl Default for Priority {
    fn default() -> Self {
        Self::NonCritical(NonZero::<u32>::MIN)
    }
}

impl PartialOrd for Priority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Priority {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Priority::Critical, Priority::Critical) => Ordering::Equal,
            (Priority::Critical, Priority::NonCritical(_)) => Ordering::Greater,
            (Priority::NonCritical(_), Priority::Critical) => Ordering::Less,
            (Priority::NonCritical(p1), Priority::NonCritical(p2)) => p1.cmp(p2),
        }
    }
}

impl Add for Priority {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Self::NonCritical(p1), Self::NonCritical(p2)) => {
                Self::NonCritical(p1.saturating_add(p2.get()))
            }
            _ => Self::Critical,
        }
    }
}

impl AddAssign for Priority {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

/// A generic marker for all graphics jobs.
#[derive(Component, Default)]
#[require(JobPriority)]
pub struct JobMarker;

/// Sets the execution priority for a scheduled job.
#[derive(Copy, Clone, Component, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct JobPriority(pub Priority);

impl JobPriority {
    #[inline(always)]
    pub const fn critical() -> Self {
        Self(Priority::Critical)
    }

    #[inline(always)]
    pub const fn non_critical<const WEIGHT: u32>() -> Self {
        const {
            assert!(WEIGHT > 0);
            //SAFETY: WEIGHT is not zero
            Self(Priority::NonCritical(unsafe {
                NonZero::new_unchecked(WEIGHT)
            }))
        }
    }

    #[inline]
    pub fn is_critical(&self) -> bool {
        self.0 == Priority::Critical
    }
}

pub(super) fn extract_job_meta(
    jobs: Extract<Query<(RenderEntity, &JobPriority), Added<JobMarker>>>,
    mut commands: Commands,
) {
    for (render_entity, priority) in &jobs {
        commands.entity(render_entity).insert(*priority);
    }
}

#[cfg(test)]
mod test {
    use std::{iter, num::NonZero};

    use super::Priority;

    fn or_min(num: u32) -> NonZero<u32> {
        NonZero::new(num).unwrap_or(NonZero::<u32>::MIN)
    }

    fn non_criticals(weights: impl IntoIterator<Item = u32>) -> Vec<Priority> {
        weights
            .into_iter()
            .map(|num| Priority::NonCritical(or_min(num)))
            .collect()
    }

    fn sum_priorities(priorities: impl IntoIterator<Item = Priority>) -> Option<Priority> {
        priorities.into_iter().reduce(|a, b| a + b)
    }

    #[test]
    fn priority_sum() {
        let priorities = non_criticals([1, 2, 3, 4, 5]);
        let sum = sum_priorities(priorities).unwrap();
        assert_eq!(sum, Priority::NonCritical(or_min(15)));
    }

    #[test]
    fn priority_sum_ones() {
        const COUNT: u32 = 20;
        let priorities = non_criticals(iter::repeat(1).take(COUNT as usize));
        let sum = sum_priorities(priorities).unwrap();
        assert_eq!(sum, Priority::NonCritical(or_min(COUNT)));
    }

    #[test]
    fn priority_sum_critical_left() {
        const COUNT: u32 = 20;
        let priorities = non_criticals(iter::repeat(1).take(COUNT as usize));
        let sum = sum_priorities(iter::once(Priority::Critical).chain(priorities)).unwrap();
        assert_eq!(sum, Priority::Critical);
    }

    #[test]
    fn priority_sum_critical_right() {
        const COUNT: u32 = 20;
        let priorities = non_criticals(iter::repeat(1).take(COUNT as usize));
        let sum =
            sum_priorities(priorities.into_iter().chain(iter::once(Priority::Critical))).unwrap();
        assert_eq!(sum, Priority::Critical);
    }
}

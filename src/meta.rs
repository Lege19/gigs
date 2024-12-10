use std::{
    cmp::Ordering,
    iter::Sum,
    num::NonZero,
    ops::{Add, AddAssign},
};

use bevy_ecs::{component::Component, entity::EntityHash};
use bevy_utils::hashbrown;

use super::JobId;

#[derive(Component, Default)]
#[require(JobPriority, ComputedPriority, JobDependencies)]
pub struct JobMarker;

#[derive(Copy, Clone, Component, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct JobPriority(pub Priority);

#[derive(Copy, Clone, Component, Default, PartialEq, Eq)]
pub struct ComputedPriority {
    priority: Priority,
    stall_frames: u32,
}

impl ComputedPriority {
    pub fn priority(&self) -> Priority {
        self.priority
    }

    pub fn is_critical(&self) -> bool {
        self.priority == Priority::Critical
    }
}

impl PartialOrd for ComputedPriority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ComputedPriority {
    fn cmp(&self, other: &Self) -> Ordering {
        todo!()
    }
}

/// The priority level of a graphics job.
///
/// Jobs with [`JobPriority::NonCritical`] will be executed in order of priority,
/// from highest to lowest.
///
/// Jobs with [`JobPriority::Critical`] will be executed *during the current frame*.
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

#[derive(Clone, Default, Component)]
pub struct JobDependencies(pub(super) hashbrown::HashSet<JobId, EntityHash>);

impl FromIterator<JobId> for JobDependencies {
    fn from_iter<T: IntoIterator<Item = JobId>>(iter: T) -> Self {
        Self(hashbrown::HashSet::from_iter(iter))
    }
}

impl JobDependencies {
    pub fn new(iter: impl IntoIterator<Item = JobId>) -> Self {
        Self::from_iter(iter)
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
        let sum =
            sum_priorities(iter::once(Priority::Critical).chain(priorities.into_iter())).unwrap();
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

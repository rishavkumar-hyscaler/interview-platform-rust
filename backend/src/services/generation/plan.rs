//! Plan the complete workload once, before paid question drafting. Reuse it on retries.
use super::types::{Blueprint, Difficulty, Kind};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub(super) struct Allocation {
    pub minutes: i32,
    pub scope: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Plan {
    pub duration_minutes: i32,
    pub blueprint: Blueprint,
    pub allocations: Vec<Allocation>,
}
pub(super) fn question_count(requested: usize, duration: i32) -> usize {
    requested.min((duration.max(0) / 2) as usize)
}
impl Plan {
    pub fn build(base: &Blueprint, duration: i32, saved: &[i32]) -> Result<Self> {
        ensure!(
            saved.len() <= base.slots.len(),
            "Saved questions exceed the blueprint"
        );
        let remaining = duration - saved.iter().sum::<i32>();
        ensure!(
            remaining >= 0,
            "Saved questions exceed the interview duration"
        );
        let unsaved_count = (base.slots.len() - saved.len()).min((remaining / 2) as usize);
        ensure!(
            unsaved_count > 0,
            "No time remains for a new 2–3 minute question; saved questions are preserved"
        );
        let mut indices: Vec<usize> = (saved.len()..base.slots.len()).collect();
        let weight = |i: usize| {
            base.competencies
                .iter()
                .find(|c| c.id == base.slots[i].competency_id)
                .map(|c| c.weight)
                .unwrap_or(0)
        };
        indices.sort_by_key(|i| (std::cmp::Reverse(weight(*i)), *i));
        indices.truncate(unsaved_count);
        indices.extend(0..saved.len());
        indices.sort_unstable();
        let mut blueprint = base.clone();
        blueprint.slots = indices
            .iter()
            .map(|i| base.slots[*i].clone())
            .collect();
        blueprint.competencies.retain(|c| {
            blueprint
                .slots
                .iter()
                .any(|s| s.competency_id == c.id)
        });
        // Renormalize retained competency weights; keep the same evidence and identifiers.
        let total_weight: u32 = blueprint
            .competencies
            .iter()
            .map(|c| c.weight)
            .sum();
        ensure!(total_weight > 0, "Plan has no competency weights");
        let mut assigned_weight = 0;
        for c in &mut blueprint.competencies {
            c.weight = (c.weight * 100 / total_weight).max(1);
            assigned_weight += c.weight;
        }
        let primary = blueprint
            .competencies
            .iter_mut()
            .max_by_key(|c| c.weight)
            .unwrap();
        primary.weight = (primary.weight as i32 + 100 - assigned_weight as i32) as u32;
        let count = blueprint.slots.len();
        if saved.is_empty() {
            for (i, slot) in blueprint.slots.iter_mut().enumerate() {
                slot.difficulty = if i < count / 3 {
                    Difficulty::Easy
                } else if i >= count - count / 3 {
                    Difficulty::Hard
                } else {
                    Difficulty::Medium
                };
            }
        }
        let mut allocations: Vec<Allocation> = blueprint.slots.iter().enumerate().map(|(i, slot)| Allocation {
            minutes: saved.get(i).copied().unwrap_or(2),
            scope: match slot.kind {
                Kind::Text => "one concept and one brief example; 2–4 sentences",
                Kind::Scenario => "one bounded decision with stated facts; 2–4 sentences of reasoning",
                Kind::Code => "one small function or a single bug fix in supplied starter code; no complete component, service or workflow",
            }.into(),
        }).collect();
        let mut extra = duration
            - allocations
                .iter()
                .map(|a| a.minutes)
                .sum::<i32>();
        let mut priority: Vec<usize> = (saved.len()..count).collect();
        priority.sort_by_key(|i| {
            let slot = &blueprint.slots[*i];
            let kind = match slot.kind {
                Kind::Text => 0,
                Kind::Scenario => 1,
                Kind::Code => 2,
            };
            let difficulty = match slot.difficulty {
                Difficulty::Easy => 0,
                Difficulty::Medium => 1,
                Difficulty::Hard => 2,
            };
            (std::cmp::Reverse(kind + difficulty), *i)
        });
        for i in priority {
            if extra > 0 {
                allocations[i].minutes = 3;
                extra -= 1;
            }
        }
        let plan = Self {
            duration_minutes: duration,
            blueprint,
            allocations,
        };
        plan.validate(saved)?;
        Ok(plan)
    }
    pub fn validate(&self, saved: &[i32]) -> Result<()> {
        ensure!(
            self.blueprint.slots.len() == self.allocations.len(),
            "Plan slot count is invalid"
        );
        ensure!(
            saved.len() <= self.allocations.len(),
            "Plan cannot discard saved questions"
        );
        for (i, allocation) in self.allocations.iter().enumerate() {
            if let Some(minutes) = saved.get(i) {
                ensure!(
                    allocation.minutes == *minutes,
                    "Saved question time changed after planning"
                );
            } else {
                ensure!(
                    (2..=3).contains(&allocation.minutes),
                    "New questions must be planned for 2–3 minutes"
                );
            }
        }
        ensure!(
            self.allocations
                .iter()
                .map(|a| a.minutes)
                .sum::<i32>()
                <= self.duration_minutes,
            "Planned workload exceeds interview duration"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generation::types::{Competency, Slot};
    fn blueprint() -> Blueprint {
        Blueprint {
            competencies: vec![Competency {
                id: "skill".into(),
                name: "Skill".into(),
                jd_evidence: "Job skill evidence".into(),
                weight: 100,
            }],
            slots: (0..6)
                .map(|i| Slot {
                    competency_id: "skill".into(),
                    kind: if i % 2 == 0 { Kind::Text } else { Kind::Code },
                    difficulty: if i < 2 {
                        Difficulty::Easy
                    } else if i < 4 {
                        Difficulty::Medium
                    } else {
                        Difficulty::Hard
                    },
                })
                .collect(),
        }
    }
    #[test]
    fn short_interviews_reduce_count_before_drafting() {
        assert_eq!(question_count(6, 10), 5);
        for duration in [5, 10, 12, 15, 30] {
            let plan = Plan::build(&blueprint(), duration, &[]).unwrap();
            assert_eq!(plan.allocations.len(), question_count(6, duration));
            assert!(
                plan.allocations
                    .iter()
                    .all(|a| (2..=3).contains(&a.minutes))
            );
            assert!(
                plan.allocations
                    .iter()
                    .map(|a| a.minutes)
                    .sum::<i32>()
                    <= duration
            );
            plan.blueprint
                .validate("Job skill evidence", plan.allocations.len())
                .unwrap();
        }
    }
    #[test]
    fn difficult_code_gets_extra_time_before_easy_text() {
        let plan = Plan::build(&blueprint(), 13, &[]).unwrap();
        assert_eq!(plan.allocations[5].minutes, 3);
        assert_eq!(plan.allocations[0].minutes, 2);
        let longer = Plan::build(&blueprint(), 30, &[]).unwrap();
        assert!(
            longer
                .allocations
                .iter()
                .all(|a| a.minutes == 3)
        );
    }
    #[test]
    fn old_checkpoints_are_preserved_and_remaining_work_is_bounded() {
        let plan = Plan::build(&blueprint(), 10, &[1, 1, 2]).unwrap();
        assert_eq!(plan.allocations.len(), 6);
        assert_eq!(
            plan.allocations
                .iter()
                .map(|a| a.minutes)
                .collect::<Vec<_>>(),
            vec![1, 1, 2, 2, 2, 2]
        );
        assert!(Plan::build(&blueprint(), 5, &[4]).is_err());
        let restored: Plan = serde_json::from_value(serde_json::to_value(&plan).unwrap()).unwrap();
        restored
            .validate(&[1, 1, 2, 2])
            .unwrap();
    }
}

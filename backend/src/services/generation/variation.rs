//! Deterministic per-session variation: each candidate's set seed picks a framing,
//! a phrasing voice, a neutral setting and concrete parameter values for every slot,
//! so two sessions for the same JD paraphrase and parameterize the same competency
//! differently. Exact and semantic deduplication still reject anything too close.
use super::types::Kind;
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub(super) struct Parameter {
    pub value: u64,
    pub suggested_use: &'static str,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub(super) struct Variation {
    pub framing: &'static str,
    pub voice: &'static str,
    pub setting: &'static str,
    pub parameters: Vec<Parameter>,
    /// Scenario and code tasks must use at least one value; text questions may.
    pub parameters_required: bool,
}

const TEXT_FRAMINGS: &[&str] = &[
    "direct conceptual question",
    "explain the idea to a new teammate",
    "compare two approaches and pick one",
    "justify a decision you would make",
    "correct a colleague's mistaken claim",
    "describe how you would verify the behaviour",
];
const SCENARIO_FRAMINGS: &[&str] = &[
    "an issue happening right now",
    "a stakeholder request with a constraint",
    "a planning trade-off before launch",
    "a review after something went wrong",
    "two requirements that conflict",
    "a handover from another team",
];
const CODE_FRAMINGS: &[&str] = &[
    "fix a failing test in supplied code",
    "complete one small helper function",
    "review and correct a short snippet",
    "adapt a working function to a new constraint",
    "make a supplied function handle an edge case",
];
const VOICES: &[&str] = &[
    "second-person instruction",
    "a direct question",
    "a short narrative opening followed by the ask",
    "a message from a colleague asking for help",
];
const SETTINGS: &[&str] = &[
    "online retail",
    "clinic appointment scheduling",
    "parcel logistics",
    "online learning",
    "travel booking",
    "subscription billing",
    "media streaming",
    "public library services",
    "factory operations",
    "nonprofit fundraising",
    "food delivery",
    "property rentals",
];

/// Ranges keep values plausible for any role while differing between sessions.
const PARAMETERS: &[(u64, u64, &str)] = &[
    (
        120,
        9800,
        "a volume such as records, users, orders, tickets or requests",
    ),
    (2, 48, "a time limit in minutes, hours or days"),
    (3, 40, "a percentage, rate or threshold"),
    (
        3,
        12,
        "a small count such as items, steps, retries or people",
    ),
];

struct Picker([u8; 32], usize);
impl Picker {
    fn next(&mut self, modulus: u64) -> u64 {
        let index = self.1 % 30;
        self.1 += 2;
        u64::from(u16::from_be_bytes([self.0[index], self.0[index + 1]])) % modulus.max(1)
    }
}

/// `round` changes after a duplicate rejection so the replacement draft differs too.
pub(super) fn for_slot(seed: Uuid, position: usize, kind: &Kind, round: u32) -> Variation {
    let digest: [u8; 32] = Sha256::digest(format!("variation-v1:{seed}:{position}:{round}")).into();
    let mut pick = Picker(digest, 0);
    let framings = match kind {
        Kind::Text => TEXT_FRAMINGS,
        Kind::Scenario => SCENARIO_FRAMINGS,
        Kind::Code => CODE_FRAMINGS,
    };
    let framing = framings[pick.next(framings.len() as u64) as usize];
    let voice = VOICES[pick.next(VOICES.len() as u64) as usize];
    let setting = SETTINGS[pick.next(SETTINGS.len() as u64) as usize];
    let parameters = PARAMETERS
        .iter()
        .map(|(low, high, suggested_use)| Parameter {
            value: low + pick.next(high - low + 1),
            suggested_use,
        })
        .collect();
    Variation {
        framing,
        voice,
        setting,
        parameters,
        parameters_required: !matches!(kind, Kind::Text),
    }
}

/// Whole numbers written in the prompt, ignoring thousands separators ("3,400").
fn numbers(prompt: &str) -> Vec<u64> {
    let mut found = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = prompt.chars().collect();
    for (index, c) in chars.iter().enumerate() {
        let separator = matches!(c, ',' | '_')
            && !current.is_empty()
            && chars
                .get(index + 1)
                .is_some_and(char::is_ascii_digit);
        if c.is_ascii_digit() {
            current.push(*c);
        } else if !separator && !current.is_empty() {
            found.extend(current.parse::<u64>().ok());
            current.clear();
        }
    }
    found.extend(current.parse::<u64>().ok());
    found
}

impl Variation {
    pub fn applied(&self, prompt: &str) -> bool {
        let used = numbers(prompt);
        self.parameters
            .iter()
            .any(|parameter| used.contains(&parameter.value))
    }
    pub fn check(&self, prompt: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.parameters_required || self.applied(prompt),
            "Question prompt must use at least one assigned variation parameter value verbatim ({}) as a concrete input, limit or test value",
            self.parameters
                .iter()
                .map(|p| p.value.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_seed_is_stable_and_different_seeds_vary() {
        let seed = Uuid::new_v4();
        assert_eq!(
            for_slot(seed, 1, &Kind::Scenario, 0),
            for_slot(seed, 1, &Kind::Scenario, 0)
        );
        let distinct: std::collections::HashSet<String> = (0..40)
            .map(|_| format!("{:?}", for_slot(Uuid::new_v4(), 1, &Kind::Scenario, 0)))
            .collect();
        assert!(distinct.len() > 35);
        assert_ne!(
            for_slot(seed, 1, &Kind::Scenario, 0),
            for_slot(seed, 1, &Kind::Scenario, 1)
        );
    }
    #[test]
    fn parameters_stay_in_range() {
        for _ in 0..200 {
            let variation = for_slot(Uuid::new_v4(), 3, &Kind::Code, 0);
            for (parameter, (low, high, _)) in variation
                .parameters
                .iter()
                .zip(PARAMETERS)
            {
                assert!((*low..=*high).contains(&parameter.value));
            }
        }
    }
    #[test]
    fn parameter_use_is_checked_with_separators() {
        let mut variation = for_slot(Uuid::new_v4(), 1, &Kind::Scenario, 0);
        variation.parameters = vec![Parameter {
            value: 3400,
            suggested_use: "volume",
        }];
        assert!(
            variation
                .check("Handle 3,400 orders per hour")
                .is_ok()
        );
        assert!(
            variation
                .check("Handle 34000 orders")
                .is_err()
        );
        assert!(
            variation
                .check("Handle many orders")
                .is_err()
        );
        variation.parameters_required = false;
        assert!(
            variation
                .check("Handle many orders")
                .is_ok()
        );
    }
}

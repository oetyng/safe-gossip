// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::id::Id;
use std::collections::BTreeMap;

/// This represents the state of a single rumor from this player's perspective.
#[derive(Serialize, Debug, Deserialize, Clone, PartialEq)]
pub enum State {
    /// A - Startup Phase.
    /// The startup phase starts in the round in which the rumor is created and _ends with the first round after whose
    /// execution there are at least ln(n)^4 informed players for the first time_. <= NB!
    /// [...] Thus O(ln(ln(n)) rounds are sufficient to achieve ln(n)^4 informed players.

    /// Exponential-growth phase.
    B {
        /// The round number for this rumor.  This is not a globally-synchronised variable, rather
        /// it is set to 0 when we first receive a copy of this rumor and is incremented every
        /// time `next_round()` is called.
        round: Round,
        /// Our age for this rumor.  This may increase by 1 during a single round or may
        /// remain the same depending on the ages attached to incoming copies of this rumor.
        age: Age,
        /// The map of <player, age>s which have sent us this rumor during this round.
        player_ages: BTreeMap<Id, Age>,
    },
    /// Quadratic-shrinking phase.
    C {
        /// The number of rounds performed by the player while the rumor was in state B.
        rounds_in_state_b: Round,
        /// The round number for this rumor while in state C.
        round: Round,
    },
    /// Propagation complete.
    D,
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    /// Construct a new `State` where we're the initial player for the rumor.  We start in
    /// state B with `age` set to `1`.
    pub fn new() -> Self {
        State::B {
            round: Round::default(),
            age: Age::from(1),
            player_ages: BTreeMap::new(),
        }
    }

    /// Construct a new `State` where we've received the rumor from a player.  If that player
    /// is in state B (`age < max_b_age`) we start in state B with `age` set to `1`.
    /// If the player is in state C, we start in state C too.
    pub fn new_from_player(player_age: Age, max_b_age: Age) -> Self {
        if player_age < max_b_age {
            return State::B {
                round: Round::default(),
                age: Age::from(1),
                player_ages: BTreeMap::new(),
            };
        }
        State::C {
            rounds_in_state_b: Round::default(),
            round: Round::default(),
        }
    }

    /// Receive a copy of this rumor from `player_id` with `age`.
    pub fn receive_rumor(&mut self, player_id: Id, age: Age) {
        if let State::B {
            ref mut player_ages,
            ..
        } = *self
        {
            if player_ages.insert(player_id, age).is_some() {
                debug!("Received the same rumor more than once this round from a given player");
                // "this" round? that's not quite correctly formulated, is it? the vec follows over multiple rounds, no?
            }
        }
    }

    /// Increment `round` value, consuming `self` and returning the new state.
    pub fn next_round(
        self,
        age_max: Age,
        max_c_rounds: Round,
        max_rounds: Round,
        //players_in_this_round: &BTreeSet<Id>,
    ) -> State {
        match self {
            State::B {
                mut round,
                mut age,
                // mut player_ages,
                player_ages,
            } => {
                round += Round::from(1);
                // If we've hit the maximum permitted number of rounds, transition to state D
                if round >= max_rounds {
                    return State::D;
                }

                // This is commented out since it seems to be dealing with updating
                // ongoing Rumors with changes in the cluster, i.e. new players.
                // // // For any `players_in_this_round` which aren't accounted for in `player_ages`, add
                // // // a age of `0` for them to indicate they're in state A (i.e. they didn't have
                // // // the rumor).
                // // for player in players_in_this_round {
                // //     if let Entry::Vacant(entry) = player_ages.entry(*player) {
                // //         let _ = entry.insert(Age::new());
                // //     }
                // // }

                // Apply the median rule, but if any player's age >= `age_max` (i.e. that player
                // is in state C), transition to state C.
                let mut less = 0;
                let mut greater_or_equal = 0;
                for player_age in player_ages.values() {
                    if *player_age < age {
                        less += 1;
                    } else if *player_age >= age_max {
                        return State::C {
                            rounds_in_state_b: round,
                            round: Round::default(),
                        };
                    } else {
                        greater_or_equal += 1;
                    }
                }
                if greater_or_equal > less {
                    age += Age::from(1);
                }

                // If our age has reached `age_max`, transition to state C, otherwise remain
                // in state B.
                if age >= age_max {
                    return State::C {
                        rounds_in_state_b: round,
                        round: Round::default(),
                    };
                }
                State::B {
                    round,
                    age,
                    player_ages: BTreeMap::new(),
                }
            }
            State::C {
                rounds_in_state_b,
                mut round,
            } => {
                round += Round::from(1);
                // If we've hit the maximum permitted number of rounds, transition to state D
                if round + rounds_in_state_b >= max_rounds {
                    return State::D;
                }

                // If we've hit the maximum rounds for remaining in state C, transition to state D.
                if round >= max_c_rounds {
                    return State::D;
                }

                // Otherwise remain in state C.
                State::C {
                    rounds_in_state_b,
                    round,
                }
            }
            State::D => State::D,
        }
    }

    /// We only need to push and pull this rumor if we're in states B or C, hence this returns
    /// `None` if we're in state D.  State C is indicated by returning a value > `age_max`.
    pub fn get_age(&self) -> Option<Age> {
        match *self {
            State::B { age, .. } => Some(age),
            State::C { .. } => Some(Age::max()),
            State::D => None,
        }
    }
}

#[derive(Copy, Clone, Serialize, Debug, Deserialize, PartialEq, PartialOrd)]
pub struct Age {
    value: u8,
}

impl Age {
    // pub fn new() -> Self {
    //     Self { value: 0 }
    // }
    pub fn from(value: u8) -> Self {
        Self { value }
    }
    pub fn max() -> Self {
        Self {
            value: u8::max_value(),
        }
    }
}

impl std::ops::AddAssign for Age {
    fn add_assign(&mut self, rhs: Self) {
        self.value += rhs.value;
    }
}

#[derive(Default, Copy, Clone, Serialize, Debug, Deserialize, PartialEq, PartialOrd)]
pub struct Round {
    value: u8,
}

impl Round {
    pub fn from(value: u8) -> Self {
        Self { value }
    }
}

impl std::ops::Add for Round {
    type Output = Round;
    fn add(self, rhs: Self) -> Round {
        Round::from(self.value + rhs.value)
    }
}

impl std::ops::AddAssign for Round {
    fn add_assign(&mut self, rhs: Self) {
        self.value += rhs.value;
    }
}

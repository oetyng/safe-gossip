// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::error::Error;
use crate::gossip::{Content, Gossip, InformedPlayer, ObliviousPlayer, Player, Rumor};
use crate::id::Id;
use crate::state::{Age, Round, State};
use rand::seq::SliceRandom;
use std::cmp;
use std::collections::{BTreeMap, BTreeSet};

type ContentHash = Vec<u8>;

/// An instance of Gossiping holds the state
/// necessary to carry out gossiping in a cluster.
pub struct Gossiping {
    our_id: Id,
    rumors: BTreeMap<ContentHash, RumorProgress>,
    players: BTreeSet<Player>,
}

impl Gossiping {
    /// Returns a new instance of the Gossiping, to be used by a player in a cluster.
    pub fn new(our_id: Id, players: BTreeSet<Player>) -> Gossiping {
        Gossiping {
            our_id,
            rumors: BTreeMap::new(),
            players,
        }
    }

    /// Our Id (i.e. its public key).
    pub fn our_id(&self) -> Id {
        self.our_id
    }

    /// Adds a player. This does not affect any ongoing Rumors.
    pub fn add_player(&mut self, player_id: Id) -> Result<(), Error> {
        // Inserting to set, so no need to check player is not already here.
        // todo: do not discard result
        let _ = self.players.insert(Player { id: player_id });

        // We just play out all rounds and disregard from any membership change whilst in them,
        // therefore, the below is commented out (and shall be removed).
        // // for ongoing in self.rumors {
        // //     ongoing.1.oblivious_players.push(player_id);
        // // }

        // If we did update rumors above, then this event basically would qualify as a trigger IMO.
        // // send_gossip();

        Ok(())
    }

    /// Removes a player. This does not affect any ongoing Rumors.
    pub fn remove_player(&mut self, player_id: Id) {
        self.players = self
            .players
            .iter()
            .filter(|c| c.id != player_id)
            .copied()
            .collect();

        // We just play out all rounds and disregard from any membership change whilst in them,
        // therefore, the below is commented out (and shall be removed).
        // // for ongoing in self.rumors {
        // //     // remove the player from any of the lists
        // //     ongoing.1.oblivious_players = ongoing.1
        // //         .oblivious_players
        // //         .iter()
        // //         .filter(|x| x.id != player_id)
        // //         .collect();
        // //     ongoing.1
        // //         .informed_players
        // //         .iter()
        // //         .filter(|x| x.id != player_id)
        // //         .collect();
        // // }
    }

    /// Initiates a rumor, which means sending it to some player.
    /// If no players, we will just hold on to the rumor until we know of any players.
    pub fn initiate_rumor(&mut self, content: Content) -> Result<(), Error> {
        let id = self.hash(content.clone());
        let cluster_size = self.players.len() as f64;

        if self
            .rumors
            .insert(
                id,
                RumorProgress {
                    content,
                    informed_players: vec![],
                    oblivious_players: self
                        .players
                        .iter()
                        .map(|c| ObliviousPlayer { id: c.id })
                        .collect(),
                    state: State::new(),
                    max_b_age: Age::from(cmp::max(1, cluster_size.ln().ceil() as u8)),
                    max_rounds: Round::from(cmp::max(1, cluster_size.ln().ln().ceil() as u8)),
                    max_c_rounds: Round::from(cmp::max(1, cluster_size.ln().ln().ceil() as u8)),
                },
            )
            .is_some()
        {
            error!("New messages should be unique.");
        };

        // This here is basically when we would trigger,
        // but we defer, and let outer layer decide when to trigger new round.

        Ok(())
    }

    /// Incoming rumors is a trigger of sending all rumors that this player has.
    pub fn receive_gossip(&mut self, gossip: &Gossip, is_push: bool) -> Option<Gossip> {
        let oblivious_players: Vec<ObliviousPlayer> = self
            .players
            .iter()
            .filter(|c| c.id != gossip.caller.id)
            .map(|c| ObliviousPlayer { id: c.id })
            .collect();

        let cluster_size = self.players.len() as f64;
        let max_b_age = Age::from(cmp::max(1, cluster_size.ln().ceil() as u8));
        let max_rounds = Round::from(cmp::max(1, cluster_size.ln().ln().ceil() as u8));

        // if we already have this rumor, update with the incoming rumor age/state
        for rumor in gossip.rumors.to_vec() {
            let id = self.hash(rumor.content.clone());
            // todo: do not discard result.
            let _ = self
                .rumors
                .entry(id)
                .and_modify(|e| {
                    e.state
                        .receive_rumor(rumor.caller.id, rumor.state.get_age().unwrap())
                })
                .or_insert(RumorProgress {
                    content: rumor.content.clone(),
                    informed_players: vec![InformedPlayer {
                        id: rumor.caller.id,
                    }], // potential tweak: include their view of this
                    oblivious_players: oblivious_players.iter().copied().collect(),
                    state: State::new_from_player(rumor.state.get_age().unwrap(), max_b_age),
                    max_b_age,
                    max_rounds,
                    max_c_rounds: max_rounds,
                });
        }

        self.try_get_response(gossip, is_push)

        // This here is basically when we would trigger,
        // but we defer, and let outer layer decide when to trigger new round.
    }

    fn try_get_response(&mut self, gossip: &Gossip, is_push: bool) -> Option<Gossip> {
        if !is_push {
            return None;
        }
        // To follow the median-rule within state B,
        // we send back our counter, to allow the caller to evolve.
        let our_id = self.our_id();
        let caller = ObliviousPlayer {
            id: gossip.caller.id,
        };
        let mut gossip = Gossip {
            callee: caller,
            rumors: gossip
                .rumors
                .to_vec()
                .into_iter()
                .map(|c| {
                    let id = self.hash(c.content);
                    let ongoing = self.rumors.get(&id).unwrap();
                    Rumor {
                        content: ongoing.content.clone(),
                        callee: caller,
                        state: ongoing.state.clone(),
                        caller: InformedPlayer { id: our_id },
                    }
                })
                .collect(),
            caller: InformedPlayer { id: our_id },
        };

        // todo: fix reuse of code from collect_gossip(&mut self)
        // We also include any rumors we think it doesn't have.
        // (This will be a distinct set from the ones we received, since we have already registered the receival).
        // Exclude any rumors which are completed (in state D).
        let active_rumors = &mut self.rumors.iter_mut().filter(|(_, c)| c.state != State::D);
        active_rumors.for_each(|(_, mut ongoing)| {
            // Each rumor has its own cycle of rounds.
            ongoing.state = ongoing.state.clone().next_round(
                ongoing.max_b_age,
                ongoing.max_c_rounds,
                ongoing.max_rounds,
            );

            let exists = ongoing
                .oblivious_players
                .iter()
                .copied()
                .find(|c| c.id == caller.id);

            let callee = match exists {
                Some(c) => c,
                None => return,
            };

            let rumor = Rumor {
                content: ongoing.content.clone(),
                callee,
                state: ongoing.state.clone(),
                caller: InformedPlayer { id: our_id },
            };

            gossip.rumors.push(rumor);

            // Move the player from Oblivious to Informed.
            ongoing.oblivious_players = ongoing
                .oblivious_players
                .iter()
                .filter(|c| c.id != callee.id)
                .copied()
                .collect();
            ongoing
                .informed_players
                .push(InformedPlayer { id: callee.id });
        });

        if !gossip.rumors.is_empty() {
            return Some(gossip);
        }
        None
    }

    /// This moves each Rumor state to next round,
    /// returning the single Gossip to send to another Player,
    /// (whom we believe to be an ObliviousPlayer, for all Rumors in this Gossip).
    pub fn collect_gossip(&mut self) -> Option<Gossip> {
        let our_id = self.our_id();

        // Exclude any rumors which are completed (in state D).
        let active_rumors = &mut self.rumors.iter_mut().filter(|(_, c)| c.state != State::D);

        let rng = &mut rand::thread_rng(); // put rng as a field of Gossiping instance instead?
        let players: Vec<Player> = self.players.iter().copied().collect();

        // Shuffle players, send to the first of them that
        // has any rumors we think it hasn't seen, and then break.
        // (We only want to send to one player at a time.)
        // This results in always sending to a Player, if at least
        // one of them is believed to be oblivious about
        // a Rumor that is not yet completed.
        for player in players.choose_multiple(rng, players.len()) {
            let mut gossip = Gossip {
                callee: ObliviousPlayer { id: player.id },
                rumors: vec![],
                caller: InformedPlayer { id: our_id },
            };

            active_rumors.for_each(|(_, mut ongoing)| {
                // Each rumor has its own cycle of rounds.
                ongoing.state = ongoing.state.clone().next_round(
                    ongoing.max_b_age,
                    ongoing.max_c_rounds,
                    ongoing.max_rounds,
                );

                let exists = ongoing
                    .oblivious_players
                    .iter()
                    .copied()
                    .find(|c| c.id == player.id);

                let callee = match exists {
                    Some(c) => c,
                    None => return,
                };

                let rumor = Rumor {
                    content: ongoing.content.clone(),
                    callee,
                    state: ongoing.state.clone(),
                    caller: InformedPlayer { id: our_id },
                };

                gossip.rumors.push(rumor);

                // Move the player from Oblivious to Informed.
                ongoing.oblivious_players = ongoing
                    .oblivious_players
                    .iter()
                    .filter(|c| c.id != callee.id)
                    .copied()
                    .collect();
                ongoing
                    .informed_players
                    .push(InformedPlayer { id: callee.id });
            });

            if !gossip.rumors.is_empty() {
                return Some(gossip);
            }
        }
        None
    }

    fn hash(&mut self, content: Content) -> Vec<u8> {
        content.value // todo
    }
}

pub struct RumorProgress {
    content: Content,
    informed_players: Vec<InformedPlayer>,
    oblivious_players: Vec<ObliviousPlayer>,
    state: State,
    // When in state B, if our age for a Rumor is incremented to this value, the state
    // transitions to C.  Specified in the paper as `O(ln ln n)`.
    max_b_age: Age,
    // The maximum number of rounds to remain in state C for a given Rumor.  Specified in the
    // paper as `O(ln ln n)`.
    max_c_rounds: Round,
    // The maximum total number of rounds for a Rumor to remain in states B or C.  This is a
    // failsafe to allow the definite termination of a Rumor being propagated.  Specified in the
    // paper as `O(ln n)`.
    max_rounds: Round,
}

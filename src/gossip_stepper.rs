// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::error::Error;
use crate::gossiping::Content;
use crate::gossiping::Gossip;
use crate::gossiping::Gossiping;
use crate::id::Id;
use futures::{Async, Future, Poll};
use std::collections::BTreeMap;

pub trait PlayerChannel {
    fn receive_from_player(&mut self) -> Option<Gossip>;
    fn send_to_player(&mut self, gossip: Gossip);
}

pub trait ClientChannel {
    fn read_from_client(&mut self) -> Option<Content>;
}

pub struct Player {}
pub struct Client {}

// todo: quic-p2p
impl PlayerChannel for Player {
    fn receive_from_player(&mut self) -> Option<Gossip> {
        // todo: impl
        None
    }

    fn send_to_player(&mut self, gossip: Gossip) {
        // todo: impl
        println!("{:?}", gossip.callee.id.0);
    }
}

impl ClientChannel for Client {
    fn read_from_client(&mut self) -> Option<Content> {
        // todo: impl
        Some(Content::new(vec![0]))
    }
}

impl Future for GossipStepper<Client, Player> {
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<(), Error> {
        if self.abort() {
            return Ok(Async::Ready(()));
        }

        self.read_from_client();
        self.receive_from_players();
        self.try_send_gossip();

        Ok(Async::NotReady)
    }
}

struct GossipStepper<Client, Player> {
    gossiping: Gossiping,
    client: Client,
    players: BTreeMap<Id, Player>,
    is_processing: bool,
}

impl GossipStepper<Client, Player> {
    /// Currently unused.
    // fn new(gossiping: Gossiping, client: Client, players: BTreeMap<Id, Player>) -> Self {
    //     Self {
    //         gossiping,
    //         client,
    //         players,
    //         is_processing: false,
    //     }
    // }

    fn abort(&mut self) -> bool {
        false
    }

    fn read_from_client(&mut self) {
        if let Some(content) = self.client.read_from_client() {
            // todo: do not discard result
            let _ = self.gossiping.initiate_rumor(content);
        }
    }

    /// Iterate the players reading any new messages from them.
    fn receive_from_players(&mut self) {
        let mut has_response = false;
        let message_streams = &mut self.players.values_mut();
        for stream in message_streams {
            if let Some(gossip) = stream.receive_from_player() {
                has_response = true;
                self.gossiping.receive_gossip(gossip);
            }
        }
        self.is_processing = has_response;
    }

    /// Tries to trigger a new push round.
    fn try_send_gossip(&mut self) {
        if self.is_processing {
            return;
        }
        if let Some(gossip) = self.gossiping.collect_gossip() {
            self.is_processing = true;

            if let Some(message_stream) = self.players.get_mut(&gossip.callee.id) {
                message_stream.send_to_player(gossip);
            }
        }
    }
}

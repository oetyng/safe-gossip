// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::error::Error;
use crate::gossip::Content;
use crate::gossiping::Gossiping;
use crate::id::Id;
use crate::transmission::Transmission;
use ed25519_dalek::Keypair;
use ed25519_dalek::PublicKey;
use futures::{Async, Future, Poll};
use std::collections::BTreeMap;

/// Defines the communication interface between
/// players in this gossip protocol.
/// Should allow for implementation of any transport protocol.
pub trait PlayerIncomingChannel {
    /// Receives rumors from other players.
    fn receive_from_players(&mut self) -> Vec<(PublicKey, Vec<u8>)>;
}

/// Defines the communication interface between
/// players in this gossip protocol.
/// Should allow for implementation of any transport protocol.
pub trait PlayerOutgoingChannels {
    /// Sends rumors to other player,
    fn send_to_player(&mut self, id: Id, transmission: (PublicKey, Vec<u8>)) -> Result<(), Error>;
}

/// Defines the communication interface between
/// the user and this instance of the gossip protocol.
/// Should allow for implementation of any transport protocol.
pub trait ClientChannel {
    /// Reads any input from user.
    fn read_from_client(&mut self) -> Option<ClientCmd>;
}

/// A cmd sent by the
/// user of this protocol.
pub enum ClientCmd {
    /// Starts a new rumor.
    NewRumor(Content),
    /// Shuts down this instance.
    Shutdown,
}

// todo: quic-p2p impl

impl<C, I, O> Future for GossipStepper<C, I, O>
where
    C: ClientChannel,
    I: PlayerIncomingChannel,
    O: PlayerOutgoingChannels,
{
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<(), Error> {
        if self.abort() {
            return Ok(Async::Ready(()));
        }

        self.read_from_client()?;
        self.receive_from_players()?;
        self.try_send_gossip()?;

        Ok(Async::NotReady)
    }
}

/// Used to carry out gossiping.
pub struct GossipStepper<C, I, O> {
    keys: Keypair,
    gossiping: Gossiping,
    client: C,
    listener: I,
    players: BTreeMap<Id, O>,
    is_processing: bool,
    is_aborted: bool,
    _p_c: std::marker::PhantomData<C>,
    _p_i: std::marker::PhantomData<I>,
    _p_o: std::marker::PhantomData<O>,
}

impl<C, I, O> GossipStepper<C, I, O>
where
    C: ClientChannel,
    I: PlayerIncomingChannel,
    O: PlayerOutgoingChannels,
{
    /// Constructor
    pub fn new(
        keys: Keypair,
        gossiping: Gossiping,
        client: C,
        listener: I,
        players: BTreeMap<Id, O>,
    ) -> Self {
        Self {
            keys,
            gossiping,
            client,
            listener,
            players,
            is_processing: false,
            is_aborted: false,
            _p_c: std::marker::PhantomData,
            _p_i: std::marker::PhantomData,
            _p_o: std::marker::PhantomData,
        }
    }

    /// Returns the Id of this instance.
    pub fn our_id(&mut self) -> Id {
        self.gossiping.our_id()
    }

    /// Adds a player to the gossip cluster.
    pub fn add_player(&mut self, public_key: PublicKey, channel: O) -> Result<(), Error> {
        let id = Id::from(public_key);
        self.gossiping.add_player(id)?;
        // todo: don't discard result
        let _ = self.players.insert(id, channel);
        Ok(())
    }

    /// Removes a player from the gossip cluster.
    pub fn remove_player(&mut self, _public_key: PublicKey) {
        // todo
    }

    fn abort(&mut self) -> bool {
        self.is_aborted
    }

    fn read_from_client(&mut self) -> Result<(), Error> {
        if let Some(cmd) = self.client.read_from_client() {
            match cmd {
                ClientCmd::NewRumor(content) => self.gossiping.initiate_rumor(content)?,
                ClientCmd::Shutdown => self.is_aborted = true,
            }
        }
        Ok(())
    }

    /// Iterate the players reading any new messages from them.
    fn receive_from_players(&mut self) -> Result<(), Error> {
        let mut has_response = false;
        for (public_key, bytes) in self.listener.receive_from_players() {
            has_response = true;
            let mut transmission = Transmission::deserialise(&bytes[..], &public_key)?;
            let (gossip, is_push) = transmission.get_value()?;
            if let Some(response) = self.gossiping.receive_gossip(&gossip, is_push) {
                let result = Transmission::serialise(&response, false, &self.keys);
                let stream_result = self.players.get_mut(&Id::from(public_key));
                match stream_result {
                    Some(stream) => {
                        stream.send_to_player(gossip.callee.id, (self.keys.public, result?))?
                    }
                    None => continue,
                }
            }
        }
        self.is_processing = has_response;
        Ok(())
    }

    /// Tries to trigger a new push round.
    fn try_send_gossip(&mut self) -> Result<(), Error> {
        if self.is_processing {
            return Ok(());
        }
        if let Some(gossip) = self.gossiping.collect_gossip() {
            self.is_processing = true;

            if let Some((_, stream)) = self
                .players
                .iter_mut()
                .find(|(c, _)| **c == gossip.callee.id)
            {
                let result = Transmission::serialise(&gossip, true, &self.keys);
                stream.send_to_player(gossip.callee.id, (self.keys.public, result?))?;
            }
        }
        Ok(())
    }
}

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

pub trait PlayerChannel {
    fn receive_from_player(&mut self) -> Option<Vec<u8>>;
    fn send_to_player(&mut self, transmission: Vec<u8>);
}

pub trait ClientChannel {
    fn read_from_client(&mut self) -> Option<Content>;
}

pub struct Player {}
pub struct Client {}

// todo: quic-p2p
impl PlayerChannel for Player {
    fn receive_from_player(&mut self) -> Option<Vec<u8>> {
        // todo: impl
        None
    }

    fn send_to_player(&mut self, transmission: Vec<u8>) {
        // todo: impl
        println!("{:?}", transmission);
    }
}

impl ClientChannel for Client {
    fn read_from_client(&mut self) -> Option<Content> {
        // todo: impl
        Some(Content { value: vec![0] })
    }
}

impl<C, P> Future for GossipStepper<C, P>
where
    C: ClientChannel,
    P: PlayerChannel,
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

struct GossipStepper<C, P> {
    keys: Keypair,
    gossiping: Gossiping,
    client: Client,
    players: BTreeMap<PublicKey, Player>,
    is_processing: bool,
    _p_c: std::marker::PhantomData<C>,
    _p_p: std::marker::PhantomData<P>,
}

impl<C, P> GossipStepper<C, P>
where
    C: ClientChannel,
    P: PlayerChannel,
{
    // Currently unused.
    // fn new(
    //     keys: Keypair,
    //     gossiping: Gossiping,
    //     client: Client,
    //     players: BTreeMap<PublicKey, Player>,
    // ) -> Self {
    //     Self {
    //         keys,
    //         gossiping,
    //         client,
    //         players,
    //         is_processing: false,
    //         _p_c: std::marker::PhantomData,
    //         _p_p: std::marker::PhantomData,
    //     }
    // }

    fn abort(&mut self) -> bool {
        // todo: receive abort msg from somewhere
        false
    }

    fn read_from_client(&mut self) -> Result<(), Error> {
        if let Some(content) = self.client.read_from_client() {
            self.gossiping.initiate_rumor(content)?
        }
        Ok(())
    }

    /// Iterate the players reading any new messages from them.
    fn receive_from_players(&mut self) -> Result<(), Error> {
        let mut has_response = false;
        let message_streams = &mut self.players.iter_mut();
        for (public_key, stream) in message_streams {
            if let Some(bytes) = stream.receive_from_player() {
                has_response = true;
                let mut transmission = Transmission::deserialise(&bytes[..], public_key)?;
                let (gossip, is_push) = transmission.get_value()?;
                if let Some(response) = self.gossiping.receive_gossip(gossip, is_push) {
                    let result = Transmission::serialise(&response, false, &self.keys);
                    stream.send_to_player(result?);
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
                .find(|(c, _)| Id(c.to_bytes()) == gossip.callee.id)
            {
                let result = Transmission::serialise(&gossip, true, &self.keys);
                stream.send_to_player(result?);
            }
        }
        Ok(())
    }
}

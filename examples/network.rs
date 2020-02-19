// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

//! Run a local network of gossiper nodes.

#![forbid(
    exceeding_bitshifts,
    mutable_transmutes,
    no_mangle_const_items,
    unknown_crate_types
)]
#![deny(
    bad_style,
    improper_ctypes,
    missing_docs,
    non_shorthand_field_patterns,
    overflowing_literals,
    stable_features,
    unconditional_recursion,
    unknown_lints,
    unsafe_code,
    unused_allocation,
    unused_attributes,
    unused_comparisons,
    unused_features,
    unused_parens,
    while_true,
    unused
)]
#![warn(
    trivial_casts,
    trivial_numeric_casts,
    unused_extern_crates,
    unused_import_braces,
    unused_qualifications,
    unused_results
)]
#![allow(
    box_pointers,
    missing_copy_implementations,
    missing_debug_implementations,
    variant_size_differences,
    non_camel_case_types
)]

use rand;
#[macro_use]
extern crate unwrap;
//use bincode::{deserialize, serialize};
use ed25519_dalek::{Keypair, PublicKey};
use futures::sync::mpsc;
use futures::{Async, Future, Poll, Stream};
use futures_cpupool::{CpuFuture, CpuPool};
use rand::distributions::Alphanumeric;
use rand::Rng;
use safe_gossip::{
    ClientChannel, ClientCmd, Content, Error, GossipStepper, Gossiping, Id, Player,
    PlayerIncomingChannel, PlayerOutgoingChannels,
};
use sha3::Sha3_512;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Debug, Formatter};
use std::iter::Iterator;
use std::mem;

///
pub struct TestPlayerIncomingChannel {
    receiver: mpsc::UnboundedReceiver<(PublicKey, Vec<u8>)>,
}

impl TestPlayerIncomingChannel {
    fn new(receiver: mpsc::UnboundedReceiver<(PublicKey, Vec<u8>)>) -> Self {
        Self { receiver }
    }
}

impl PlayerIncomingChannel for TestPlayerIncomingChannel {
    fn receive_from_players(&mut self) -> Vec<(PublicKey, Vec<u8>)> {
        let mut incoming = vec![];
        while let Async::Ready(Some(message)) = unwrap!(self.receiver.poll()) {
            incoming.push(message);
        }
        incoming
    }
}

///
#[derive(Clone)]
pub struct TestPlayerOutgoingChannels {
    senders: BTreeMap<Id, mpsc::UnboundedSender<(PublicKey, Vec<u8>)>>,
}

impl TestPlayerOutgoingChannels {
    fn new(senders: BTreeMap<Id, mpsc::UnboundedSender<(PublicKey, Vec<u8>)>>) -> Self {
        Self { senders }
    }
}

impl PlayerOutgoingChannels for TestPlayerOutgoingChannels {
    fn send_to_player(&mut self, id: Id, transmission: (PublicKey, Vec<u8>)) -> Result<(), Error> {
        unwrap!(unwrap!(self.senders.get_mut(&id)).unbounded_send(transmission));
        Ok(())
    }
}

/// Receives cmds from user.
pub struct TestClientChannel {
    receiver: mpsc::UnboundedReceiver<ClientCmd>,
}

impl TestClientChannel {
    fn new(receiver: mpsc::UnboundedReceiver<ClientCmd>) -> Self {
        Self { receiver }
    }
}

impl ClientChannel for TestClientChannel {
    fn read_from_client(&mut self) -> Option<ClientCmd> {
        if let Async::Ready(Some(cmd)) = unwrap!(self.receiver.poll()) {
            return Some(cmd);
        }
        None
    }
}

struct Network {
    pool: CpuPool,
    // An mpsc channel sender for each node for giving new client messages to that node.
    client_senders: BTreeMap<Id, mpsc::UnboundedSender<ClientCmd>>,
    // The futures for all nodes.  When these return ready, that node has finished running.
    node_futures: Vec<CpuFuture<(), Error>>,
    // Stats
    stats: Stats,
}

impl Network {
    fn new(node_count: usize) -> Self {
        let mut players = BTreeSet::new();
        let mut player_senders = BTreeMap::new();
        let mut client_senders = BTreeMap::new();
        let mut player_receivers = BTreeMap::new();
        let mut client_receivers = BTreeMap::new();
        let mut player_infos = BTreeMap::new();
        for _ in 0..node_count {
            let mut rng = rand::thread_rng();
            let keys = Keypair::generate::<Sha3_512, _>(&mut rng);
            let (player_sender, player_receiver) = mpsc::unbounded();
            let (client_sender, client_receiver) = mpsc::unbounded();
            let id = Id::from(keys.public);

            let _ = player_infos.insert(id, keys);
            let _ = players.insert(Player { id });
            let _ = player_senders.insert(id, player_sender);
            let _ = client_senders.insert(id, client_sender);
            let _ = player_receivers.insert(id, player_receiver);
            let _ = client_receivers.insert(id, client_receiver);
        }

        let player_channels = player_infos
            .values_mut()
            .map(|k| {
                let channel = TestPlayerOutgoingChannels::new(player_senders.clone());
                (Id::from(k.public), channel)
            })
            .collect::<BTreeMap<Id, TestPlayerOutgoingChannels>>();

        let mut nodes = vec![];
        for (id, keys) in player_infos {
            let node = GossipStepper::new(
                keys,
                Gossiping::new(id, players.clone()),
                TestClientChannel::new(unwrap!(client_receivers.remove(&id))),
                TestPlayerIncomingChannel::new(unwrap!(player_receivers.remove(&id))),
                player_channels.clone(),
            );
            nodes.push(node);
        }

        //nodes.sort_by(|lhs, rhs| lhs.our_id().cmp(&rhs.our_id()));

        let mut network = Network {
            // pool: CpuPool::new(1),
            pool: CpuPool::new_num_cpus(),
            client_senders,
            node_futures: vec![],
            stats: Stats::new(),
        };

        // Start the nodes running by executing their `poll()` functions on the threadpool.
        for node in nodes {
            network.node_futures.push(network.pool.spawn(node));
        }

        network
    }

    /// Send the given `message`.  If `node_index` is `Some` and is less than the number of `Node`s
    /// in the `Network`, then the `Node` at that index will be chosen as the initial informed one.
    fn send(&mut self, message: &str, node_index: Option<usize>) -> Result<(), Error> {
        let i = match node_index {
            Some(index) if index < self.client_senders.len() => index,
            _ => rand::thread_rng().gen_range(0, self.client_senders.len()),
        };
        let cmd = ClientCmd::NewRumor(Content {
            value: String::from(message).into_bytes(),
        });
        unwrap!(self.client_senders.values_mut().collect::<Vec<_>>()[i].unbounded_send(cmd));
        Ok(())
    }

    fn reached_all_players(&mut self) -> bool {
        // todo
        false
    }
}

impl Future for Network {
    type Item = Stats;
    type Error = String;

    fn poll(&mut self) -> Poll<Stats, String> {
        if self.reached_all_players() {
            return Ok(Async::Ready(self.stats.clone()));
        }

        Ok(Async::NotReady)
    }
}

impl Drop for Network {
    fn drop(&mut self) {
        for sender in &mut self.client_senders.values_mut() {
            unwrap!(sender.unbounded_send(ClientCmd::Shutdown));
        }
        let node_futures = mem::replace(&mut self.node_futures, vec![]);
        for node_future in node_futures {
            unwrap!(node_future.wait());
        }
    }
}

fn main() {
    let num_of_nodes = 16;
    let num_of_extra_msgs = 0;
    println!("Number of extra msgs to input {:?}", num_of_extra_msgs);

    let mut polls = vec![];
    let mut sent = vec![];

    for i in 0..100 {
        println!("Sim iter {:?}", i);
        let stats = run(num_of_nodes, num_of_extra_msgs);
        polls.push(stats.clone().poll_count);
        sent.push(stats.clone().sent_count);
    }

    println!("Average poll count {:?}", average(&polls[..]));
    println!("Median poll count {:?}", median(&mut polls[..]));

    println!("Average sent count {:?}", average(&sent[..]));
    println!("Median sent count {:?}", median(&mut sent[..]));
}

fn run(num_of_nodes: u64, num_of_extra_msgs: u64) -> Stats {
    let mut network = Network::new(num_of_nodes as usize);
    unwrap!(network.send("Hello", None));
    unwrap!(network.send("there", Some(999)));
    unwrap!(network.send("world", Some(0)));
    unwrap!(network.send("!", Some(0)));

    // A real network continues to send messages..

    let mut rng = rand::thread_rng();

    let mut messages: Vec<String> = Vec::new();
    for _ in 0..num_of_extra_msgs {
        let msg = rng.sample_iter(&Alphanumeric).take(10).collect::<String>();
        messages.push(msg);
    }

    for msg in messages {
        unwrap!(network.send(&msg[..], Some(0)));
    }

    unwrap!(network.pool.clone().spawn(network).wait())
}

fn average(numbers: &[u64]) -> f32 {
    numbers.iter().sum::<u64>() as f32 / numbers.len() as f32
}

fn median(numbers: &mut [u64]) -> u64 {
    numbers.sort();
    let mid = numbers.len() / 2;
    numbers[mid]
}

/// Statistics on each network sim.
#[derive(Clone, Default)]
pub struct Stats {
    /// Number of polls done
    pub poll_count: u64,
    /// Number of total messages sent
    pub sent_count: u64,
}

impl Stats {
    /// Create a default
    pub fn new() -> Self {
        Stats {
            poll_count: 0,
            sent_count: 0,
        }
    }
}

impl Debug for Stats {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "poll_count: {},  sent_count: {}, ",
            self.poll_count, self.sent_count,
        )
    }
}

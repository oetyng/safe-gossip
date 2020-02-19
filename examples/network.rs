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

//use futures::try_ready;
use rand;
//#[macro_use]
//extern crate tokio_io;
#[macro_use]
extern crate unwrap;
//use bincode::{deserialize, serialize};
use futures::sync::mpsc;
use futures::{Async, Future, Poll, Stream};
use futures_cpupool::{CpuFuture, CpuPool};
use safe_gossip::{
    ClientChannel, ClientCmd, Content, Error, GossipStepper, Gossiping, Id, Player, PlayerChannel,
};

// use crate::error::Error;
// use crate::gossip::{Content, Player};
// use crate::gossip_stepper::{ClientChannel, ClientCmd, GossipStepper, PlayerChannel};
// use crate::gossiping::Gossiping;
// use crate::id::Id;

use sha3::Sha3_512;
//use itertools::Itertools;
use rand::distributions::Alphanumeric;
use rand::Rng;
//use std::cell::RefCell;
//use std::collections::HashMap;
use std::fmt::{self, Debug, Formatter};
//use std::io::Write;
use std::iter::Iterator;
use std::mem;
//use std::rc::Rc;
//use std::thread;
//use tokio::executor::current_thread;
//use tokio_io::AsyncRead;
use ed25519_dalek::Keypair;
//use ed25519_dalek::PublicKey;
use std::collections::{BTreeMap, BTreeSet};

pub struct TestPlayerChannel {
    receiver: mpsc::UnboundedReceiver<Vec<u8>>,
    senders: BTreeMap<Id, mpsc::UnboundedSender<Vec<u8>>>,
}

impl TestPlayerChannel {
    fn new(
        receiver: mpsc::UnboundedReceiver<Vec<u8>>,
        senders: BTreeMap<Id, mpsc::UnboundedSender<Vec<u8>>>,
    ) -> Self {
        Self { senders, receiver }
    }
}

impl PlayerChannel for TestPlayerChannel {
    fn receive_from_player(&mut self) -> Option<Vec<u8>> {
        while let Async::Ready(Some(message)) = unwrap!(self.receiver.poll()) {
            return Some(message);
        }
        None
    }

    fn send_to_player(&mut self, id: Id, transmission: Vec<u8>) -> Result<(), Error> {
        unwrap!(unwrap!(self.senders.get_mut(&id)).unbounded_send(transmission));
        Ok(())
    }
}

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
        while let Async::Ready(Some(cmd)) = unwrap!(self.receiver.poll()) {
            return Some(cmd);
        }
        None
    }
}

struct Info {
    player: Player,
    keys: Keypair,
    player_sender: mpsc::UnboundedSender<Vec<u8>>,
    player_receiver: mpsc::UnboundedReceiver<Vec<u8>>,
    client_sender: mpsc::UnboundedSender<ClientCmd>,
    client_receiver: mpsc::UnboundedReceiver<ClientCmd>,
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
        let players = BTreeSet::new();
        let mut player_senders = BTreeMap::new();
        let mut client_senders = BTreeMap::new();
        let mut player_infos = BTreeMap::new();
        for _ in 0..node_count {
            let mut rng = rand::thread_rng();
            let keys = Keypair::generate::<Sha3_512, _>(&mut rng);
            let (player_sender, player_receiver) = mpsc::unbounded();
            let (client_sender, client_receiver) = mpsc::unbounded();
            let id = Id::from(keys.public);

            player_infos.insert(
                id,
                Info {
                    player: Player { id },
                    keys,
                    player_sender: player_sender.clone(),
                    player_receiver: player_receiver,
                    client_sender: client_sender.clone(),
                    client_receiver: client_receiver,
                },
            );

            player_senders.insert(id, player_sender);
            client_senders.insert(id, client_sender);
        }

        let mut player_channels = vec![];
        for (_, player) in player_infos {
            let channel = TestPlayerChannel::new(player.player_receiver, player_senders.clone());
            player_channels.push((player.keys.public, channel));
        }

        let mut nodes = vec![];
        for (id, player) in player_infos {
            let node = GossipStepper::new(
                player.keys,
                Gossiping::new(id, players.clone()),
                TestClientChannel::new(player.client_receiver),
                player_channels.clone(),
            );
            nodes.push(node);
        }

        nodes.sort_by(|lhs, rhs| lhs.our_id().cmp(&rhs.our_id()));

        let mut rng = rand::thread_rng();
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
        for (_, sender) in &mut self.client_senders {
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

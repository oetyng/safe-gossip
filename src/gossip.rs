// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::id::Id;
use crate::state::State;

#[derive(Serialize, Debug, Deserialize)]
pub struct Gossip {
    pub callee: ObliviousPlayer,
    pub rumors: Vec<Rumor>,
    pub caller: InformedPlayer,
}

#[derive(Serialize, Debug, Deserialize, Clone)]
pub struct Rumor {
    pub content: Content,
    pub callee: ObliviousPlayer,
    pub state: State,
    pub caller: InformedPlayer,
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub struct Player {
    pub id: Id,
}

#[derive(Serialize, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub struct InformedPlayer {
    pub id: Id,
}

#[derive(Serialize, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub struct ObliviousPlayer {
    pub id: Id,
}

#[derive(Clone, Serialize, Debug, Deserialize)]
pub struct Content {
    pub value: Vec<u8>,
}

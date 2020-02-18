// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::error::Error;
use crate::gossip::Gossip;
use bincode::{deserialize, serialize};
use ed25519_dalek::{Keypair, PublicKey, Signature};
#[cfg(not(test))]
use sha3::Sha3_512;

#[derive(Debug, Serialize, Deserialize)]
pub enum Transmission {
    /// Sent from Node A to Node B to push a message and its counter.
    Push { payload: Vec<u8>, sig: Signature },
    /// NOTE: Called Pull in the paper.
    /// Sent from Node B to Node A as a reaction to receiving a push message from A.
    Response { payload: Vec<u8>, sig: Signature },
}

/// Transmission via direct connection, wrapper of gossip.
#[cfg(not(test))]
impl Transmission {
    pub fn get_value(&mut self) -> Result<(Gossip, bool), Error> {
        match self {
            Self::Push { payload, .. } => Ok((deserialize(payload)?, true)),
            Self::Response { payload, .. } => Ok((deserialize(payload)?, false)),
        }
    }

    pub fn serialise(gossip: &Gossip, is_push: bool, keys: &Keypair) -> Result<Vec<u8>, Error> {
        let payload = serialize(gossip)?;
        let sig: Signature = keys.sign::<Sha3_512>(&payload);
        let transmission = if is_push {
            Transmission::Push { payload, sig }
        } else {
            Transmission::Response { payload, sig }
        };
        Ok(serialize(&transmission)?)
    }

    pub fn deserialise(payload: &[u8], key: &PublicKey) -> Result<Transmission, Error> {
        let mut transmission: Transmission = deserialize(payload)?;
        transmission.verify_sig(key)?;
        Ok(transmission)
    }

    fn verify_sig(&mut self, key: &PublicKey) -> Result<(), Error> {
        let (payload, sig) = match self {
            Transmission::Push { payload, sig } => (payload, sig),
            Transmission::Response { payload, sig } => (payload, sig),
        };
        if key.verify::<Sha3_512>(&payload, &sig).is_ok() {
            Ok(())
        } else {
            Err(Error::SigFailure)
        }
    }
}

#[cfg(test)]
impl Transmission {
    pub fn get_value(&mut self) -> Result<(Gossip, bool), Error> {
        match self {
            Self::Push { payload, .. } => Ok((deserialize(payload)?, true)),
            Self::Response { payload, .. } => Ok((deserialize(payload)?, false)),
        }
    }

    pub fn serialise(gossip: &Gossip, _is_push: bool, _keys: &Keypair) -> Result<Vec<u8>, Error> {
        Ok(serialize(gossip)?)
    }

    pub fn deserialise(payload: &[u8], _key: &PublicKey) -> Result<Transmission, Error> {
        Ok(deserialize(&payload)?)
    }
}

// RGB Standard Library: high-level API to RGB smart contracts.
// Written in 2019-2022 by
//     Dr. Maxim Orlovsky <orlovsky@lnp-bp.org>
//
// To the extent possible under law, the author(s) have dedicated all copyright
// and related and neighboring rights to this software to the public domain
// worldwide. This software is distributed without any warranty.
//
// You should have received a copy of the MIT License along with this software.
// If not, see <https://opensource.org/licenses/MIT>.

use std::collections::{BTreeMap, BTreeSet};
use std::convert::TryInto;
use std::io::Read;
use std::str::FromStr;

use bitcoin::hashes::{sha256, sha256t};
use bitcoin::Txid;
use commit_verify::{
    commit_encode, lnpbp4, CommitConceal, CommitVerify, ConsensusCommit, PrehashedProtocol,
    TaggedHash,
};
use lnpbp_bech32::{self, FromBech32Str, ToBech32String};
use strict_encoding::{LargeVec, StrictDecode};

use crate::{
    schema, seal, Anchor, BundleId, ConcealSeals, ConcealState, ConsistencyError, Extension,
    Genesis, GraphApi, Node, NodeId, Schema, SealEndpoint, Transition, TransitionBundle,
};

pub type ConsignmentEndpoints = Vec<(BundleId, SealEndpoint)>;
pub type AnchoredBundles = LargeVec<(Anchor<lnpbp4::MerkleProof>, TransitionBundle)>;
pub type ExtensionList = LargeVec<Extension>;

pub const RGB_CONSIGNMENT_VERSION: u8 = 0;

static MIDSTATE_CONSIGNMENT_ID: [u8; 32] = [
    8, 36, 37, 167, 51, 70, 76, 241, 171, 132, 169, 56, 76, 108, 174, 226, 197, 98, 75, 254, 29,
    125, 170, 233, 184, 121, 13, 183, 90, 51, 134, 6,
];

/// Tag used for [`ConsignmentId`] hash types
pub struct ConsignmentIdTag;

impl sha256t::Tag for ConsignmentIdTag {
    #[inline]
    fn engine() -> sha256::HashEngine {
        let midstate = sha256::Midstate::from_inner(MIDSTATE_CONSIGNMENT_ID);
        sha256::HashEngine::from_midstate(midstate, 64)
    }
}

/// Unique consignment identifier equivalent to the commitment hash
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize), serde(crate = "serde_crate"))]
#[derive(Wrapper, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Display, From)]
#[derive(StrictEncode, StrictDecode)]
#[wrapper(Debug, LowerHex, Index, IndexRange, IndexFrom, IndexTo, IndexFull)]
#[display(ConsignmentId::to_bech32_string)]
pub struct ConsignmentId(sha256t::Hash<ConsignmentIdTag>);

impl<Msg> CommitVerify<Msg, PrehashedProtocol> for ConsignmentId
where Msg: AsRef<[u8]>
{
    #[inline]
    fn commit(msg: &Msg) -> ConsignmentId { ConsignmentId::hash(msg) }
}

impl commit_encode::Strategy for ConsignmentId {
    type Strategy = commit_encode::strategies::UsingStrict;
}

impl lnpbp_bech32::Strategy for ConsignmentId {
    const HRP: &'static str = "id";
    type Strategy = lnpbp_bech32::strategies::UsingStrictEncoding;
}

impl FromStr for ConsignmentId {
    type Err = lnpbp_bech32::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> { ConsignmentId::from_bech32_str(s) }
}

/// Consignment represents contract-specific data, always starting with genesis,
/// which must be valid under client-side-validation rules (i.e. internally
/// consistent and properly committed into the commitment layer, like bitcoin
/// blockchain or current state of the lightning channel).
///
/// All consignment-related procedures, including validation or merging
/// consignment data into stash or schema-specific data storage, must start with
/// `endpoints` and process up to the genesis. If any of the nodes within the
/// consignment are not part of the paths connecting endpoints with the genesis,
/// consignment validation will return
/// [`crate::validation::Warning::ExcessiveNode`] warning
#[cfg_attr(
    all(feature = "cli", feature = "serde"),
    derive(Serialize, Deserialize),
    serde(crate = "serde_crate")
)]
#[derive(Clone, PartialEq, Eq, Debug, StrictEncode)]
pub struct FullConsignment {
    /// Version, used internally
    version: u8,

    pub schema: Schema,

    /// Genesis data
    pub genesis: Genesis,

    /// The final state ("endpoints") provided by this consignment.
    ///
    /// There are two reasons for having endpoints:
    /// - navigation towards genesis from the final state is more
    ///   computationally efficient, since state transition/extension graph is
    ///   directed towards genesis (like bitcoin transaction graph)
    /// - if the consignment contains concealed state (known by the receiver),
    ///   it will be computationally inefficient to understand which of the
    ///   state transitions represent the final state
    pub endpoints: ConsignmentEndpoints,

    /// Data on all anchored state transitions contained in the consignment
    pub anchored_bundles: AnchoredBundles,

    /// Data on all state extensions contained in the consignment
    pub state_extensions: ExtensionList,
}

impl commit_encode::Strategy for FullConsignment {
    type Strategy = commit_encode::strategies::UsingStrict;
}

impl ConsensusCommit for FullConsignment {
    type Commitment = ConsignmentId;
}

impl StrictDecode for FullConsignment {
    fn strict_decode<D: Read>(mut d: D) -> Result<Self, strict_encoding::Error> {
        let consignment = strict_decode_self!(d; version, schema, genesis, endpoints, anchored_bundles, state_extensions);
        if consignment.version != 0 {
            return Err(strict_encoding::Error::UnsupportedDataStructure(
                "Consignment versions above 0 are not supported",
            ));
        }
        Ok(consignment)
    }
}

// TODO #60: Implement different conceal procedures for the consignment

impl FullConsignment {
    #[inline]
    pub fn with(
        schema: Schema,
        genesis: Genesis,
        endpoints: ConsignmentEndpoints,
        anchored_bundles: AnchoredBundles,
        state_extensions: ExtensionList,
    ) -> FullConsignment {
        Self {
            version: RGB_CONSIGNMENT_VERSION,
            schema,
            genesis,
            endpoints,
            state_extensions,
            anchored_bundles,
        }
    }

    #[inline]
    pub fn id(&self) -> ConsignmentId { self.clone().consensus_commit() }

    #[inline]
    pub fn version(&self) -> u8 { self.version }

    #[inline]
    pub fn txids(&self) -> BTreeSet<Txid> {
        self.anchored_bundles
            .iter()
            .map(|(anchor, _)| anchor.txid)
            .collect()
    }

    #[inline]
    pub fn node_ids(&self) -> BTreeSet<NodeId> {
        let mut set = bset![self.genesis.node_id()];
        set.extend(
            self.anchored_bundles
                .iter()
                .flat_map(|(_, bundle)| bundle.known_node_ids()),
        );
        set.extend(self.state_extensions.iter().map(Extension::node_id));
        set
    }

    #[inline]
    pub fn endpoint_bundle_ids(&self) -> BTreeSet<BundleId> {
        self.endpoints
            .iter()
            .map(|(bundle_id, _)| bundle_id)
            .copied()
            .collect()
    }

    #[inline]
    pub fn endpoint_bundles(&self) -> Vec<&TransitionBundle> {
        self.endpoint_bundle_ids()
            .into_iter()
            .filter_map(|bundle_id| self.bundle_by_id(bundle_id).ok())
            .collect()
    }

    #[inline]
    pub fn endpoint_transition_by_id(
        &self,
        node_id: NodeId,
    ) -> Result<&Transition, ConsistencyError> {
        if self
            .endpoints
            .iter()
            .filter_map(|(id, _)| self.bundle_by_id(*id).ok())
            .flat_map(|bundle| bundle.known_node_ids())
            .any(|id| id == node_id)
        {
            return Err(ConsistencyError::NotEndpoint(node_id));
        }

        self.transition_by_id(node_id)
    }

    #[inline]
    pub fn endpoint_transitions_by_type(
        &self,
        transition_type: schema::TransitionType,
    ) -> Vec<&Transition> {
        self.endpoint_transitions_by_types(&[transition_type])
    }

    #[inline]
    pub fn endpoint_transitions_by_types(
        &self,
        types: &[schema::TransitionType],
    ) -> Vec<&Transition> {
        self.endpoint_bundle_ids()
            .into_iter()
            .filter_map(|bundle_id| self.known_transitions_by_bundle_id(bundle_id).ok())
            .flat_map(Vec::into_iter)
            .filter(|node| types.contains(&node.transition_type()))
            .collect()
    }

    pub fn finalize(&mut self, expose: &BTreeSet<SealEndpoint>) -> usize {
        let concealed_endpoints = expose
            .iter()
            .map(SealEndpoint::commit_conceal)
            .collect::<Vec<_>>();

        let mut removed_endpoints = vec![];
        self.endpoints = self
            .endpoints
            .clone()
            .into_iter()
            .filter(|(_, endpoint)| {
                if expose.contains(endpoint) {
                    true
                } else {
                    removed_endpoints.push(*endpoint);
                    false
                }
            })
            .collect();
        let seals_to_conceal = removed_endpoints
            .iter()
            .map(SealEndpoint::commit_conceal)
            .collect::<Vec<_>>();

        let mut count = 0usize;
        self.anchored_bundles = self
            .anchored_bundles
            .iter()
            .map(|(anchor, bundle)| {
                let bundle = bundle
                    .into_iter()
                    .map(|(transition, inputs)| {
                        let mut transition = transition.clone();
                        count += transition.conceal_state_except(&concealed_endpoints)
                            + transition.conceal_seals(&seals_to_conceal);
                        (transition, inputs.clone())
                    })
                    .collect::<BTreeMap<_, _>>();
                (anchor.clone(), TransitionBundle::from(bundle))
            })
            .collect::<Vec<_>>()
            .try_into()
            .expect("size of the original collection not changed");

        count = self
            .state_extensions
            .iter_mut()
            .fold(count, |count, extension| {
                count + extension.conceal_state_except(&concealed_endpoints)
            });

        count
    }

    /// Reveals previously known seal information (replacing blind UTXOs with
    /// unblind ones). Function is used when a peer receives consignment
    /// containing concealed seals for the outputs owned by the peer
    pub fn reveal_seals<'a>(
        &mut self,
        known_seals: impl Iterator<Item = &'a seal::Revealed> + Clone,
    ) -> usize {
        let mut counter = 0;
        for (_, bundle) in self.anchored_bundles.iter_mut() {
            *bundle = bundle
                .into_iter()
                .map(|(transition, inputs)| {
                    let mut transition = transition.clone();
                    for (_, assignment) in transition.owned_rights_mut().iter_mut() {
                        counter += assignment.reveal_seals(known_seals.clone());
                    }
                    (transition, inputs.clone())
                })
                .collect::<BTreeMap<_, _>>()
                .into();
        }
        for extension in self.state_extensions.iter_mut() {
            for (_, assignment) in extension.owned_rights_mut().iter_mut() {
                counter += assignment.reveal_seals(known_seals.clone())
            }
        }
        counter
    }
}

#[cfg(test)]
pub(crate) mod test {
    use amplify::Wrapper;
    use commit_verify::tagged_hash;
    use strict_encoding::StrictDecode;
    use wallet::onchain::TxResolverError;

    use super::*;
    use crate::schema::test::schema;

    static CONSIGNMENT: [u8; 1496] = include!("../../test/consignment.in");

    pub(crate) fn consignment() -> FullConsignment {
        FullConsignment::strict_decode(&CONSIGNMENT[..]).unwrap()
    }

    struct TestResolver;

    impl ResolveTx for TestResolver {
        fn resolve_tx(&self, txid: Txid) -> Result<bitcoin::Transaction, TxResolverError> {
            eprintln!("Validating txid {}", txid);
            Err(TxResolverError { txid, err: None })
        }
    }

    #[test]
    fn test_consignment_validation() {
        let consignment = consignment();
        let schema = schema();
        let status = consignment.validate(&schema, None, TestResolver);
        println!("{}", status);
    }

    #[test]
    fn test_consignment_id_midstate() {
        let midstate = tagged_hash::Midstate::with(b"rgb:consignment");
        assert_eq!(midstate.into_inner().into_inner(), MIDSTATE_CONSIGNMENT_ID);
    }
}

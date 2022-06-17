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

/// Iterator over transitions and corresponding witness transaction ids which
/// can be created out of consignment data. Transitions of this type must be
/// organized into a chain connecting 1-to-1 via the provided `connected_by`
/// during iterator creation.
///
/// Iterator is created with [`Consignment::chain_iter`]
#[derive(Debug)]
pub struct ChainIter<'iter> {
    consignment: &'iter Consignment,
    connected_by: OwnedRightType,
    next_item: Option<(&'iter Transition, Txid)>,
    error: Option<ConsistencyError>,
}

impl<'iter> ChainIter<'iter> {
    /// Detects whether iterator was stopped by a error
    pub fn is_err(&'iter self) -> bool { self.error.is_some() }

    /// Converts iterator into a result type prividing information about the
    /// error (if any) which terminated execution of the iterator
    pub fn into_result(self) -> Result<(), ConsistencyError> {
        if let Some(err) = self.error {
            Err(err)
        } else {
            Ok(())
        }
    }
}

impl<'iter> Iterator for ChainIter<'iter> {
    type Item = (&'iter Transition, Txid);

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.next_item?;

        let output = if let Some(output) = item
            .0
            .parent_outputs_by_type(self.connected_by)
            .first()
            .copied()
        {
            output
        } else {
            self.next_item = None;
            return None;
        };

        self.next_item = self
            .consignment
            .transition_witness_by_id(output.node_id)
            .map_err(|err| self.error = Some(err))
            .ok();

        Some(item)
    }
}

impl Consignment {
    /// Creates iterator over a single chain of state transition starting from
    /// `node_id` which must be one of the consignment endpoints, and
    /// corresponding witness transaction ids. Transitions must be organized
    /// into a chain connecting 1-to-1 via the provided `connected_by` owned
    /// rights (one or none of them must be present for each state transition).
    pub fn chain_iter(&self, start_with: NodeId, connected_by: OwnedRightType) -> ChainIter {
        let next_item = self
            .endpoint_transition_by_id(start_with)
            .into_iter()
            .next()
            .and_then(|_| self.transition_witness_by_id(start_with).ok());

        ChainIter {
            consignment: self,
            connected_by,
            next_item,
            error: None,
        }
    }

    pub fn transition_witness_iter<'iter>(
        &'iter self,
        transition_types: &'iter [TransitionType],
    ) -> MeshIter<'iter> {
        let mut bundles = self.anchored_bundles.iter();
        let next = bundles.next();
        let transitions = next.map(|(anchor, bundle)| (anchor.txid, bundle.known_transitions()));
        MeshIter {
            bundles,
            transitions,
            transition_types,
        }
    }
}

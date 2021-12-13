// Copyright 2018-2021 Cargill Incorporated
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Database backend support for the AdminServiceStore, powered by
//! [`Diesel`](https://crates.io/crates/diesel).
//!
//! This module contains the [`DieselAdminServiceStore`], which provides an implementation of the
//! [`AdminServiceStore`] trait.
//!
//! [`DieselAdminServiceStore`]: struct.DieselAdminServiceStore.html
//! [`AdminServiceStore`]: ../trait.AdminServiceStore.html

mod models;
mod operations;
mod schema;

use std::sync::{Arc, RwLock};

use diesel::r2d2::{ConnectionManager, Pool};

use crate::admin::messages;
use crate::admin::store::{
    error::AdminServiceStoreError, AdminServiceStore, Circuit, CircuitNode, CircuitPredicate,
    CircuitProposal, Service, ServiceId,
};
use crate::admin::store::{AdminServiceEvent, EventIter};
use crate::store::pool::ConnectionPool;

use operations::add_circuit::AdminServiceStoreAddCircuitOperation as _;
use operations::add_event::AdminServiceStoreAddEventOperation as _;
use operations::add_proposal::AdminServiceStoreAddProposalOperation as _;
use operations::count_circuits::AdminServiceStoreCountCircuitsOperation as _;
use operations::count_proposals::AdminServiceStoreCountProposalsOperation as _;
use operations::get_circuit::AdminServiceStoreFetchCircuitOperation as _;
use operations::get_node::AdminServiceStoreFetchNodeOperation as _;
use operations::get_proposal::AdminServiceStoreFetchProposalOperation as _;
use operations::get_service::AdminServiceStoreFetchServiceOperation as _;
use operations::list_circuits::AdminServiceStoreListCircuitsOperation as _;
use operations::list_events_by_management_type_since::AdminServiceStoreListEventsByManagementTypeSinceOperation as _;
use operations::list_events_since::AdminServiceStoreListEventsSinceOperation as _;
use operations::list_nodes::AdminServiceStoreListNodesOperation as _;
use operations::list_proposals::AdminServiceStoreListProposalsOperation as _;
use operations::list_services::AdminServiceStoreListServicesOperation as _;
use operations::remove_circuit::AdminServiceStoreRemoveCircuitOperation as _;
use operations::remove_proposal::AdminServiceStoreRemoveProposalOperation as _;
use operations::update_circuit::AdminServiceStoreUpdateCircuitOperation as _;
use operations::update_proposal::AdminServiceStoreUpdateProposalOperation as _;
use operations::upgrade::AdminServiceStoreUpgradeProposalToCircuitOperation as _;
use operations::AdminServiceStoreOperations;

/// A database-backed AdminServiceStore, powered by [`Diesel`](https://crates.io/crates/diesel).
pub struct DieselAdminServiceStore<C: diesel::Connection + 'static> {
    connection_pool: ConnectionPool<C>,
}

impl<C: diesel::Connection> DieselAdminServiceStore<C> {
    /// Creates a new `DieselAdminServiceStore`.
    ///
    /// # Arguments
    ///
    ///  * `connection_pool`: connection pool for the database
    pub fn new(connection_pool: Pool<ConnectionManager<C>>) -> Self {
        DieselAdminServiceStore {
            connection_pool: connection_pool.into(),
        }
    }

    /// Create a new `DieselAdminServiceStore` with write exclusivity enabled.
    ///
    /// Write exclusivity is enforced by providing a connection pool that is wrapped in a
    /// [`RwLock`]. This ensures that there may be only one writer, but many readers.
    ///
    /// # Arguments
    ///
    ///  * `connection_pool`: read-write lock-guarded connection pool for the database
    pub fn new_with_write_exclusivity(
        connection_pool: Arc<RwLock<Pool<ConnectionManager<C>>>>,
    ) -> Self {
        Self {
            connection_pool: connection_pool.into(),
        }
    }
}

#[cfg(feature = "sqlite")]
impl Clone for DieselAdminServiceStore<diesel::sqlite::SqliteConnection> {
    fn clone(&self) -> Self {
        Self {
            connection_pool: self.connection_pool.clone(),
        }
    }
}

#[cfg(feature = "postgres")]
impl Clone for DieselAdminServiceStore<diesel::pg::PgConnection> {
    fn clone(&self) -> Self {
        Self {
            connection_pool: self.connection_pool.clone(),
        }
    }
}

#[cfg(feature = "postgres")]
impl AdminServiceStore for DieselAdminServiceStore<diesel::pg::PgConnection> {
    fn add_proposal(&self, proposal: CircuitProposal) -> Result<(), AdminServiceStoreError> {
        self.connection_pool
            .execute_write(|conn| AdminServiceStoreOperations::new(conn).add_proposal(proposal))
    }

    fn update_proposal(&self, proposal: CircuitProposal) -> Result<(), AdminServiceStoreError> {
        self.connection_pool
            .execute_write(|conn| AdminServiceStoreOperations::new(conn).update_proposal(proposal))
    }

    fn remove_proposal(&self, proposal_id: &str) -> Result<(), AdminServiceStoreError> {
        self.connection_pool.execute_write(|conn| {
            AdminServiceStoreOperations::new(conn).remove_proposal(proposal_id)
        })
    }

    fn get_proposal(
        &self,
        proposal_id: &str,
    ) -> Result<Option<CircuitProposal>, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).get_proposal(proposal_id))
    }

    fn list_proposals(
        &self,
        predicates: &[CircuitPredicate],
    ) -> Result<Box<dyn ExactSizeIterator<Item = CircuitProposal>>, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).list_proposals(predicates))
    }

    fn count_proposals(
        &self,
        predicates: &[CircuitPredicate],
    ) -> Result<u32, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).count_proposals(predicates))
    }

    fn add_circuit(
        &self,
        circuit: Circuit,
        nodes: Vec<CircuitNode>,
    ) -> Result<(), AdminServiceStoreError> {
        self.connection_pool.execute_write(|conn| {
            AdminServiceStoreOperations::new(conn).add_circuit(circuit, nodes)
        })
    }

    fn update_circuit(&self, circuit: Circuit) -> Result<(), AdminServiceStoreError> {
        self.connection_pool
            .execute_write(|conn| AdminServiceStoreOperations::new(conn).update_circuit(circuit))
    }

    fn remove_circuit(&self, circuit_id: &str) -> Result<(), AdminServiceStoreError> {
        self.connection_pool
            .execute_write(|conn| AdminServiceStoreOperations::new(conn).remove_circuit(circuit_id))
    }

    fn get_circuit(&self, circuit_id: &str) -> Result<Option<Circuit>, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).get_circuit(circuit_id))
    }

    fn list_circuits(
        &self,
        predicates: &[CircuitPredicate],
    ) -> Result<Box<dyn ExactSizeIterator<Item = Circuit>>, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).list_circuits(predicates))
    }

    fn count_circuits(
        &self,
        predicates: &[CircuitPredicate],
    ) -> Result<u32, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).count_circuits(predicates))
    }

    fn upgrade_proposal_to_circuit(&self, circuit_id: &str) -> Result<(), AdminServiceStoreError> {
        self.connection_pool.execute_write(|conn| {
            AdminServiceStoreOperations::new(conn).upgrade_proposal_to_circuit(circuit_id)
        })
    }

    fn get_node(&self, node_id: &str) -> Result<Option<CircuitNode>, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).get_node(node_id))
    }

    fn list_nodes(
        &self,
    ) -> Result<Box<dyn ExactSizeIterator<Item = CircuitNode>>, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).list_nodes())
    }

    fn get_service(
        &self,
        service_id: &ServiceId,
    ) -> Result<Option<Service>, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).get_service(service_id))
    }

    fn list_services(
        &self,
        circuit_id: &str,
    ) -> Result<Box<dyn ExactSizeIterator<Item = Service>>, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).list_services(circuit_id))
    }

    fn add_event(
        &self,
        event: messages::AdminServiceEvent,
    ) -> Result<AdminServiceEvent, AdminServiceStoreError> {
        self.connection_pool
            .execute_write(|conn| AdminServiceStoreOperations::new(conn).add_event(event))
    }

    fn list_events_since(&self, start: i64) -> Result<EventIter, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).list_events_since(start))
    }

    fn list_events_by_management_type_since(
        &self,
        management_type: String,
        start: i64,
    ) -> Result<EventIter, AdminServiceStoreError> {
        self.connection_pool.execute_read(|conn| {
            AdminServiceStoreOperations::new(conn)
                .list_events_by_management_type_since(management_type, start)
        })
    }

    fn clone_boxed(&self) -> Box<dyn AdminServiceStore> {
        Box::new(self.clone())
    }
}

#[cfg(feature = "sqlite")]
impl AdminServiceStore for DieselAdminServiceStore<diesel::sqlite::SqliteConnection> {
    fn add_proposal(&self, proposal: CircuitProposal) -> Result<(), AdminServiceStoreError> {
        self.connection_pool
            .execute_write(|conn| AdminServiceStoreOperations::new(conn).add_proposal(proposal))
    }

    fn update_proposal(&self, proposal: CircuitProposal) -> Result<(), AdminServiceStoreError> {
        self.connection_pool
            .execute_write(|conn| AdminServiceStoreOperations::new(conn).update_proposal(proposal))
    }

    fn remove_proposal(&self, proposal_id: &str) -> Result<(), AdminServiceStoreError> {
        self.connection_pool.execute_write(|conn| {
            AdminServiceStoreOperations::new(conn).remove_proposal(proposal_id)
        })
    }

    fn get_proposal(
        &self,
        proposal_id: &str,
    ) -> Result<Option<CircuitProposal>, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).get_proposal(proposal_id))
    }

    fn list_proposals(
        &self,
        predicates: &[CircuitPredicate],
    ) -> Result<Box<dyn ExactSizeIterator<Item = CircuitProposal>>, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).list_proposals(predicates))
    }

    fn count_proposals(
        &self,
        predicates: &[CircuitPredicate],
    ) -> Result<u32, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).count_proposals(predicates))
    }

    fn add_circuit(
        &self,
        circuit: Circuit,
        nodes: Vec<CircuitNode>,
    ) -> Result<(), AdminServiceStoreError> {
        self.connection_pool.execute_write(|conn| {
            AdminServiceStoreOperations::new(conn).add_circuit(circuit, nodes)
        })
    }

    fn update_circuit(&self, circuit: Circuit) -> Result<(), AdminServiceStoreError> {
        self.connection_pool
            .execute_write(|conn| AdminServiceStoreOperations::new(conn).update_circuit(circuit))
    }

    fn remove_circuit(&self, circuit_id: &str) -> Result<(), AdminServiceStoreError> {
        self.connection_pool
            .execute_write(|conn| AdminServiceStoreOperations::new(conn).remove_circuit(circuit_id))
    }

    fn get_circuit(&self, circuit_id: &str) -> Result<Option<Circuit>, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).get_circuit(circuit_id))
    }

    fn list_circuits(
        &self,
        predicates: &[CircuitPredicate],
    ) -> Result<Box<dyn ExactSizeIterator<Item = Circuit>>, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).list_circuits(predicates))
    }

    fn count_circuits(
        &self,
        predicates: &[CircuitPredicate],
    ) -> Result<u32, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).count_circuits(predicates))
    }

    fn upgrade_proposal_to_circuit(&self, circuit_id: &str) -> Result<(), AdminServiceStoreError> {
        self.connection_pool.execute_write(|conn| {
            AdminServiceStoreOperations::new(conn).upgrade_proposal_to_circuit(circuit_id)
        })
    }

    fn get_node(&self, node_id: &str) -> Result<Option<CircuitNode>, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).get_node(node_id))
    }

    fn list_nodes(
        &self,
    ) -> Result<Box<dyn ExactSizeIterator<Item = CircuitNode>>, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).list_nodes())
    }

    fn get_service(
        &self,
        service_id: &ServiceId,
    ) -> Result<Option<Service>, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).get_service(service_id))
    }

    fn list_services(
        &self,
        circuit_id: &str,
    ) -> Result<Box<dyn ExactSizeIterator<Item = Service>>, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).list_services(circuit_id))
    }

    fn add_event(
        &self,
        event: messages::AdminServiceEvent,
    ) -> Result<AdminServiceEvent, AdminServiceStoreError> {
        self.connection_pool
            .execute_write(|conn| AdminServiceStoreOperations::new(conn).add_event(event))
    }

    fn list_events_since(&self, start: i64) -> Result<EventIter, AdminServiceStoreError> {
        self.connection_pool
            .execute_read(|conn| AdminServiceStoreOperations::new(conn).list_events_since(start))
    }

    fn list_events_by_management_type_since(
        &self,
        management_type: String,
        start: i64,
    ) -> Result<EventIter, AdminServiceStoreError> {
        self.connection_pool.execute_read(|conn| {
            AdminServiceStoreOperations::new(conn)
                .list_events_by_management_type_since(management_type, start)
        })
    }

    fn clone_boxed(&self) -> Box<dyn AdminServiceStore> {
        Box::new(self.clone())
    }
}

#[cfg(all(test, feature = "sqlite"))]
pub mod tests {
    use super::*;

    use crate::admin::store::{
        CircuitBuilder, CircuitNodeBuilder, CircuitProposal, CircuitProposalBuilder, CircuitStatus,
        ProposalType, ProposedCircuitBuilder, ProposedNodeBuilder, ProposedServiceBuilder,
        ServiceBuilder, Vote, VoteRecordBuilder,
    };

    use crate::admin::store::{AdminServiceEventBuilder, EventType};
    use crate::hex::parse_hex;
    use crate::migrations::run_sqlite_migrations;
    use crate::public_key::PublicKey;

    use diesel::{
        r2d2::{ConnectionManager, Pool},
        sqlite::SqliteConnection,
    };

    #[test]
    /// Test that the AdminServiceStore sqlite migrations can be run successfully
    fn test_sqlite_migrations() {
        create_connection_pool_and_migrate();
    }

    /// Verify that a proposal can be added to the store correctly and then fetched from the store
    ///
    /// 1. Run sqlite migrations
    /// 2. Create DieselAdminServiceStore
    /// 3. Create a proposal
    /// 4. Add proposal to store
    /// 5. Fetch Proposal from store
    /// 6. Validate fetched proposal is the same as the proposal added
    #[test]
    fn test_add_get_proposals() {
        let pool = create_connection_pool_and_migrate();

        let store = DieselAdminServiceStore::new(pool);

        let proposal = create_proposal();

        store
            .add_proposal(proposal.clone())
            .expect("Unable to add circuit proposal");

        let fetched_proposal = store
            .get_proposal("WBKLF-BBBBB")
            .expect("Unable to get proposal")
            .expect("Got None when expecting proposal");

        assert_eq!(proposal, fetched_proposal);
    }

    /// Verify that list_proposals works correctly
    ///
    /// 1. Run sqlite migrations
    /// 2. Create DieselAdminServiceStore
    /// 3. Create a proposal
    /// 4. Add proposal to store
    /// 5. List Proposal from store with no predicates, validate added proposal is returned
    /// 6. List Proposal from store with management type predicate, validate added proposal is
    ///    returned
    /// 7. List Proposal from store with member predicate, validate added proposal is
    ///    returned
    /// 8. List Proposal from store with mismatching management type predicate, validate no
    ///    proposals are returned
    #[test]
    fn test_list_proposals() {
        let pool = create_connection_pool_and_migrate();

        let store = DieselAdminServiceStore::new(pool);

        let proposal = create_proposal();

        store
            .add_proposal(proposal.clone())
            .expect("Unable to add circuit proposal");

        // test no predicates
        let mut proposals = store
            .list_proposals(&vec![])
            .expect("Unable to list proposals");

        assert_eq!(proposals.next(), Some(proposal.clone()));
        assert_eq!(proposals.next(), None);

        // test management type predicate
        let mut proposals = store
            .list_proposals(&vec![CircuitPredicate::ManagementTypeEq(
                "gameroom".to_string(),
            )])
            .expect("Unable to list proposals with management type predicate");

        assert_eq!(proposals.next(), Some(proposal.clone()));
        assert_eq!(proposals.next(), None);

        // test management type predicate
        let mut proposals = store
            .list_proposals(&vec![CircuitPredicate::ManagementTypeEq(
                "arcade".to_string(),
            )])
            .expect("Unable to list proposals with management type predicate");

        assert_eq!(proposals.next(), None);

        let extra_proposal = create_extra_proposal();

        store
            .add_proposal(extra_proposal.clone())
            .expect("Unable to add circuit proposal");

        // test management type predicate
        let mut proposals = store
            .list_proposals(&vec![CircuitPredicate::MembersInclude(vec![
                "gumbo-node-000".to_string(),
            ])])
            .expect("Unable to list proposals with members include predicate");

        assert_eq!(proposals.next(), Some(extra_proposal));
        assert_eq!(proposals.next(), None);

        let proposals = store
            .list_proposals(&vec![])
            .expect("Unable to list proposals with members include predicate");

        assert_eq!(proposals.len(), 2);
    }

    /// Verify that count_proposals works correctly
    ///
    /// 1. Run sqlite migrations
    /// 2. Create DieselAdminServiceStore
    /// 3. Create a proposal
    /// 4. Add proposal to store
    /// 5. Count Proposals in the store with no predicates, validate correct number is returned
    /// 6. Count Proposals in the store with management type predicate, validate correct number is
    ///    returned
    /// 7. Count Proposals in the store with member predicate, validate correct number is
    ///    returned
    /// 8. Count Proposal from store with mismatching management type predicate, validate 0 is
    ///    returned
    #[test]
    fn test_count_proposals() {
        let pool = create_connection_pool_and_migrate();

        let store = DieselAdminServiceStore::new(pool);

        let proposal = create_proposal();

        store
            .add_proposal(proposal.clone())
            .expect("Unable to add circuit proposal");

        // test no predicates
        assert_eq!(
            store
                .count_proposals(&vec![])
                .expect("Unable to list proposals"),
            1,
        );

        // test management type predicate
        assert_eq!(
            store
                .count_proposals(&vec![CircuitPredicate::ManagementTypeEq(
                    "gameroom".to_string(),
                )])
                .expect("Unable to list proposals"),
            1,
        );

        let extra_proposal = create_extra_proposal();

        store
            .add_proposal(extra_proposal.clone())
            .expect("Unable to add circuit proposal");

        // test member type predicate
        assert_eq!(
            store
                .count_proposals(&vec![CircuitPredicate::MembersInclude(vec![
                    "gumbo-node-000".to_string(),
                ])])
                .expect("Unable to list proposals"),
            1,
        );

        // test bad management type predicate
        assert_eq!(
            store
                .count_proposals(&vec![CircuitPredicate::ManagementTypeEq(
                    "arcade".to_string(),
                )])
                .expect("Unable to list proposals"),
            0,
        );
    }

    /// Verify that a proposal can be removed from the store
    ///
    /// 1. Run sqlite migrations
    /// 2. Create DieselAdminServiceStore
    /// 3. Create a proposal
    /// 4. Add proposal to store
    /// 5. Fetch Proposal from store
    /// 6. Validate fetched proposal is the same as the proposal added
    /// 7. Remove proposal
    /// 8. Validate the proposal was removed
    #[test]
    fn test_remove_proposals() {
        let pool = create_connection_pool_and_migrate();

        let store = DieselAdminServiceStore::new(pool);

        let proposal = create_proposal();

        store
            .add_proposal(proposal.clone())
            .expect("Unable to add circuit proposal");

        let fetched_proposal = store
            .get_proposal("WBKLF-BBBBB")
            .expect("Unable to get proposal")
            .expect("Got None when expecting proposal");

        assert_eq!(proposal, fetched_proposal);

        store
            .remove_proposal("WBKLF-BBBBB")
            .expect("Unable to add circuit proposal");

        let fetched_proposal = store
            .get_proposal("WBKLF-BBBBB")
            .expect("Unable to get proposal");

        assert_eq!(None, fetched_proposal);
    }

    /// Verify that a proposal can be added to the store correctly and then updated from the store
    ///
    /// 1. Run sqlite migrations
    /// 2. Create DieselAdminServiceStore
    /// 3. Create a proposal
    /// 4. Add proposal to store
    /// 5. Fetch Proposal from store
    /// 6. Validate fetched proposal is the same as the proposal added
    /// 7. Update proposal to have a new vote and call update
    /// 8. Fetch Proposal from store
    /// 9. Validate fetched proposal now matches the updated proposal
    #[test]
    fn test_update_proposals() {
        let pool = create_connection_pool_and_migrate();

        let store = DieselAdminServiceStore::new(pool);

        let proposal = create_proposal();

        store
            .add_proposal(proposal.clone())
            .expect("Unable to add circuit proposal");

        let fetched_proposal = store
            .get_proposal("WBKLF-BBBBB")
            .expect("Unable to get proposal")
            .expect("Got None when expecting proposal");

        assert_eq!(proposal, fetched_proposal);

        let updated_proposal = proposal
            .builder()
            .with_votes(&vec![VoteRecordBuilder::new()
                .with_public_key(&PublicKey::from_bytes(
                    parse_hex("035724d11cae47c8907f8bfdf510488f49df8494ff81b63825bad923733c4ac550")
                        .unwrap(),
                ))
                .with_vote(&Vote::Accept)
                .with_voter_node_id("bubba-node-000")
                .build()
                .expect("Unable to build vote record")])
            .build()
            .expect("Unable to build updated proposal");

        store
            .update_proposal(updated_proposal.clone())
            .expect("Unable to update proposal");

        let fetched_proposal = store
            .get_proposal("WBKLF-BBBBB")
            .expect("Unable to get proposal")
            .expect("Got None when expecting proposal");

        assert_eq!(updated_proposal, fetched_proposal);
    }

    /// Verify that a proposal can be upgraded to a circuit
    ///
    /// 1. Run sqlite migrations
    /// 2. Create DieselAdminServiceStore
    /// 3. Create a proposal
    /// 4. Add proposal to store
    /// 5. Fetch Proposal from store
    /// 6. Validate fetched proposal is the same as the proposal added
    /// 7. Call upgrade_proposal_to_circuit for the proposal
    /// 8. Fetch the new circuit and validate it is as expected
    #[test]
    fn test_upgrade_proposals() {
        let pool = create_connection_pool_and_migrate();

        let store = DieselAdminServiceStore::new(pool);

        let proposal = create_proposal();

        store
            .add_proposal(proposal.clone())
            .expect("Unable to add circuit proposal");

        let fetched_proposal = store
            .get_proposal("WBKLF-BBBBB")
            .expect("Unable to get proposal")
            .expect("Got None when expecting proposal");

        assert_eq!(proposal, fetched_proposal);

        store
            .upgrade_proposal_to_circuit("WBKLF-BBBBB")
            .expect("Unable to add circuit proposal");

        assert!(store
            .get_proposal("WBKLF-BBBBB")
            .expect("Unable to get proposal")
            .is_none());

        let fetched_circuit = store
            .get_circuit("WBKLF-BBBBB")
            .expect("Unable to get circuit")
            .expect("Got None when expecting circuit");

        assert_eq!(
            create_circuit_from_proposal("WBKLF-BBBBB", CircuitStatus::Active),
            fetched_circuit
        );
    }

    /// Verify that a circuit can be added to the store correctly and then fetched from the store
    ///
    /// 1. Run sqlite migrations
    /// 2. Create DieselAdminServiceStore
    /// 3. Create a circuit and nodes
    /// 4. Add circuit and nodes to store
    /// 5. Fetch Circuit from store
    /// 6. Validate fetched circuit is the same as the circuit added
    /// 7. Fetch CircuitNode from store
    /// 8. Validate fetched node is the same as the node added
    #[test]
    fn test_add_get_circuit_and_nodes() {
        let pool = create_connection_pool_and_migrate();

        let store = DieselAdminServiceStore::new(pool);

        let circuit = create_circuit("WBKLF-BBBBB", CircuitStatus::Active);

        let nodes = create_nodes();

        store
            .add_circuit(circuit.clone(), nodes)
            .expect("Unable to add circuit");

        let fetched_circuit = store
            .get_circuit("WBKLF-BBBBB")
            .expect("Unable to get circuit")
            .expect("Got None when expecting circuit");

        let fetched_node = store
            .get_node("bubba-node-000")
            .expect("Unable to get node")
            .expect("Got None when expecting node");

        assert_eq!(circuit, fetched_circuit);
        assert_eq!(
            fetched_node,
            CircuitNodeBuilder::default()
                .with_node_id("bubba-node-000".into())
                .with_endpoints(&vec!["tcps://splinterd-node-bubba:8044".into()])
                .build()
                .expect("Unable to build node"),
        )
    }

    /// Verify that list_circuits works correctly
    ///
    /// 1. Run sqlite migrations
    /// 2. Create DieselAdminServiceStore
    /// 3. Create a circuit and nodes
    /// 4. Add circuit to store
    /// 5. List circuits from store with no predicates, validate added circuit is returned
    /// 6. List circuits from store with management type predicate, validate added circuit is
    ///    returned
    /// 7. List circuits from store with member predicate, validate added circuit is
    ///    returned
    /// 8. List circuits from store with mismatching management type predicate, validate no
    ///    circuits are returned
    /// 9. Add a `Disbanded` circuit to the store
    /// 10. List circuits from store with no circuit status predicate, validate that only the
    ///     `Active` circuits are returned
    /// 11. List circuits with the `CircuitStatus::Disbanded` circuit status predicate, validate
    ///     only the `Disbanded` circuit is returned
    /// 12. List circuits with the `CircuitStatus::Abandoned` circuit status predicate, validate
    ///     no circuits are returned
    /// 13. List circuits from store with predicates, validate only the 2 `Active` circuits are
    ///    returned
    #[test]
    fn test_list_circuits() {
        let pool = create_connection_pool_and_migrate();

        let store = DieselAdminServiceStore::new(pool);

        let circuit = create_circuit("WBKLF-BBBBB", CircuitStatus::Active);
        let nodes = create_nodes();

        let extra_circuit = create_extra_circuit("WBKLF-CCCCC");
        let extra_nodes = create_extra_nodes();

        store
            .add_circuit(circuit.clone(), nodes.clone())
            .expect("Unable to add circuit");

        // test no predicates
        let mut circuits = store
            .list_circuits(&vec![])
            .expect("Unable to list circuits");

        assert_eq!(circuits.next(), Some(circuit.clone()));
        assert_eq!(circuits.next(), None);

        // test management type predicate
        let mut circuits = store
            .list_circuits(&vec![CircuitPredicate::ManagementTypeEq(
                "gameroom".to_string(),
            )])
            .expect("Unable to list circuits with management type predicate");

        assert_eq!(circuits.next(), Some(circuit.clone()));
        assert_eq!(circuits.next(), None);

        // test bad management type predicate
        let mut circuits = store
            .list_circuits(&vec![CircuitPredicate::ManagementTypeEq(
                "arcade".to_string(),
            )])
            .expect("Unable to list circuits with management type predicate");

        assert_eq!(circuits.next(), None);

        store
            .add_circuit(extra_circuit.clone(), extra_nodes)
            .expect("Unable to add circuit");

        // test members type predicate
        let mut circuits = store
            .list_circuits(&vec![CircuitPredicate::MembersInclude(vec![
                "gumbo-node-000".to_string(),
            ])])
            .expect("Unable to list circuits with members include predicate");

        assert_eq!(circuits.next(), Some(extra_circuit.clone()));
        assert_eq!(circuits.next(), None);

        // test circuit status predicate

        // Add a `Disbanded` circuit
        let disbanded_circuit = create_circuit("WBKLF-DDDDD", CircuitStatus::Disbanded);
        store
            .add_circuit(disbanded_circuit.clone(), nodes.clone())
            .expect("Unable to add disbanded circuit");

        // Return circuits with no predicates, this should by default only return `Active` circuits
        let mut circuits = store
            .list_circuits(&vec![])
            .expect("Unable to list circuits");

        assert_eq!(circuits.next(), Some(extra_circuit.clone()));
        assert_eq!(circuits.next(), Some(circuit.clone()));
        assert_eq!(circuits.next(), None);

        // Return circuits with the `CircuitStatus(CircuitStatus::Disbanded)` predicate
        let mut circuits = store
            .list_circuits(&vec![CircuitPredicate::CircuitStatus(
                CircuitStatus::Disbanded,
            )])
            .expect("Unable to list circuits with `CircuitStatus` predicate");

        assert_eq!(circuits.next(), Some(disbanded_circuit.clone()));
        assert_eq!(circuits.next(), None);

        // Return circuits with the `CircuitStatus(CircuitStatus::Abandoned)` predicate
        let mut circuits = store
            .list_circuits(&vec![CircuitPredicate::CircuitStatus(
                CircuitStatus::Abandoned,
            )])
            .expect("Unable to list circuits with `CircuitStatus` predicate");

        assert_eq!(circuits.next(), None);

        // show all `Active` circuits are returned
        let circuits = store
            .list_circuits(&vec![])
            .expect("Unable to list circuits");

        assert_eq!(circuits.len(), 2);
    }

    /// Verify that count_circuits works correctly
    ///
    /// 1. Run sqlite migrations
    /// 2. Create DieselAdminServiceStore
    /// 3. Create a circuit and nodes
    /// 4. Add circuit to store
    /// 5. Count circuits from store with no predicates, validated correct number is returned
    /// 6. Count circuits from store with management type predicate, validated correct number is
    ///    returned
    /// 7. Count circuits from store with member predicate, validated correct number is returned
    /// 8. Count circuits from store with mismatching management type predicate, validated 0 is
    ///    returned
    /// 9. Add a `Disbanded` circuit to the store
    /// 10. Count circuits from store with no circuit status predicate, validate that the correct
    ///     number of `Active` circuits are returned
    /// 11. Count circuits with the `CircuitStatus::Disbanded` circuit status predicate, validate
    ///     that the correct number of `Disbanded` circuits are returned
    /// 12. Count circuits with the `CircuitStatus::Abandoned` circuit status predicate, validate
    ///     that the correct number of `Abandoned` circuits are returned
    #[test]
    fn test_count_circuits() {
        let pool = create_connection_pool_and_migrate();

        let store = DieselAdminServiceStore::new(pool);

        let circuit = create_circuit("WBKLF-BBBBB", CircuitStatus::Active);
        let nodes = create_nodes();

        let extra_circuit = create_extra_circuit("WBKLF-CCCCC");
        let extra_nodes = create_extra_nodes();

        store
            .add_circuit(circuit.clone(), nodes.clone())
            .expect("Unable to add circuit");

        // test no predicates
        assert_eq!(
            store
                .count_circuits(&vec![])
                .expect("Unable to list circuits"),
            1
        );

        // test management type predicate
        assert_eq!(
            store
                .count_circuits(&vec![CircuitPredicate::ManagementTypeEq(
                    "gameroom".to_string(),
                )])
                .expect("Unable to list circuits"),
            1
        );

        // test bad management type predicate
        assert_eq!(
            store
                .count_circuits(&vec![CircuitPredicate::ManagementTypeEq(
                    "arcade".to_string(),
                )])
                .expect("Unable to list circuits"),
            0
        );

        store
            .add_circuit(extra_circuit.clone(), extra_nodes)
            .expect("Unable to add circuit");

        // test members type predicate
        assert_eq!(
            store
                .count_circuits(&vec![CircuitPredicate::MembersInclude(vec![
                    "gumbo-node-000".to_string(),
                ])])
                .expect("Unable to list circuits"),
            1
        );

        // test circuit status predicate

        // Add a `Disbanded` circuit
        let disbanded_circuit = create_circuit("WBKLF-DDDDD", CircuitStatus::Disbanded);
        store
            .add_circuit(disbanded_circuit.clone(), nodes.clone())
            .expect("Unable to add disbanded circuit");

        // Return count of circuits with no predicates, this should by default only return
        // the count of `Active` circuits
        assert_eq!(
            store
                .count_circuits(&vec![])
                .expect("Unable to list circuits"),
            2
        );

        // Return count of circuits with the `CircuitStatus(CircuitStatus::Disbanded)` predicate
        assert_eq!(
            store
                .count_circuits(&vec![CircuitPredicate::CircuitStatus(
                    CircuitStatus::Disbanded,
                )])
                .expect("Unable to list circuits"),
            1
        );

        // Return count of circuits with the `CircuitStatus(CircuitStatus::Abandoned)` predicate
        assert_eq!(
            store
                .count_circuits(&vec![CircuitPredicate::CircuitStatus(
                    CircuitStatus::Abandoned,
                )])
                .expect("Unable to list circuits"),
            0
        );
    }

    /// Verify that a circuit can be removed from the store
    ///
    /// 1. Run sqlite migrations
    /// 2. Create DieselAdminServiceStore
    /// 3. Create a circuit
    /// 4. Add circuit to store
    /// 5. Fetch circuit from store
    /// 6. Validate fetched circuit is the same as the proposal added
    /// 7. Remove circuit
    /// 8. Validate the circuit was removed
    #[test]
    fn test_remove_circuits() {
        let pool = create_connection_pool_and_migrate();

        let store = DieselAdminServiceStore::new(pool);

        let circuit = create_circuit("WBKLF-BBBBB", CircuitStatus::Active);
        let nodes = create_nodes();

        store
            .add_circuit(circuit.clone(), nodes)
            .expect("Unable to add circuit");

        let fetched_circuit = store
            .get_circuit("WBKLF-BBBBB")
            .expect("Unable to get circuit")
            .expect("Got None when expecting circuit");

        assert_eq!(circuit, fetched_circuit);

        store
            .remove_circuit("WBKLF-BBBBB")
            .expect("Unable to add circuit");

        let fetched_circuit = store
            .get_circuit("WBKLF-BBBBB")
            .expect("Unable to get circuit");

        assert_eq!(None, fetched_circuit);
    }

    /// Verify that a service can be fetched from the store
    ///
    /// 1. Run sqlite migrations
    /// 2. Create DieselAdminServiceStore
    /// 3. Create a circuit
    /// 4. Add circuit to store
    /// 5. Fetch circuit from store
    /// 6. fetch a service from the store
    #[test]
    fn test_get_service() {
        let pool = create_connection_pool_and_migrate();

        let store = DieselAdminServiceStore::new(pool);

        let circuit = create_circuit("WBKLF-BBBBB", CircuitStatus::Active);
        let nodes = create_nodes();

        store
            .add_circuit(circuit.clone(), nodes)
            .expect("Unable to add circuit");

        let fetched_circuit = store
            .get_circuit("WBKLF-BBBBB")
            .expect("Unable to get circuit")
            .expect("Got None when expecting circuit");

        assert_eq!(circuit, fetched_circuit);

        let service_id = ServiceId::new("WBKLF-BBBBB".to_string(), "a000".to_string());
        let fetched_service = store
            .get_service(&service_id)
            .expect("Unable to get service")
            .expect("Got None when expecting service");

        assert_eq!(fetched_circuit.roster()[0], fetched_service);
    }

    /// Verify that all service from a circuit can be listed from the store
    ///
    /// 1. Run sqlite migrations
    /// 2. Create DieselAdminServiceStore
    /// 3. Create a circuit
    /// 4. Add circuit to store
    /// 5. Fetch circuit from store
    /// 6. List all service from the circuit
    #[test]
    fn test_list_service() {
        let pool = create_connection_pool_and_migrate();

        let store = DieselAdminServiceStore::new(pool);

        let circuit = create_circuit("WBKLF-BBBBB", CircuitStatus::Active);
        let nodes = create_nodes();

        store
            .add_circuit(circuit.clone(), nodes)
            .expect("Unable to add circuit");

        let fetched_circuit = store
            .get_circuit("WBKLF-BBBBB")
            .expect("Unable to get circuit")
            .expect("Got None when expecting circuit");

        assert_eq!(circuit, fetched_circuit);

        let mut services = store
            .list_services("WBKLF-BBBBB")
            .expect("Unable to get services");

        assert!(fetched_circuit
            .roster()
            .contains(&services.next().expect("Unable to get service")));

        assert!(fetched_circuit
            .roster()
            .contains(&services.next().expect("Unable to get service")));

        assert_eq!(None, services.next());
    }

    /// Verify that all nodes can be listed from the store
    ///
    /// 1. Run sqlite migrations
    /// 2. Create DieselAdminServiceStore
    /// 3. Create a circuit and nodes
    /// 4. Add circuit and nodes to store
    /// 5. Fetch circuit from store
    /// 6. List all nodes from the store
    #[test]
    fn test_list_nodes() {
        let pool = create_connection_pool_and_migrate();

        let store = DieselAdminServiceStore::new(pool);

        let circuit = create_circuit("WBKLF-BBBBB", CircuitStatus::Active);
        let nodes = create_nodes();

        store
            .add_circuit(circuit.clone(), nodes)
            .expect("Unable to add circuit");

        let fetched_circuit = store
            .get_circuit("WBKLF-BBBBB")
            .expect("Unable to get circuit")
            .expect("Got None when expecting circuit");

        assert_eq!(circuit, fetched_circuit);

        let mut nodes = store.list_nodes().expect("Unable to get services");

        assert!(fetched_circuit
            .members()
            .contains(&nodes.next().expect("Unable to get service")));

        assert!(fetched_circuit
            .members()
            .contains(&nodes.next().expect("Unable to get service")));

        assert!(nodes.next().is_none());
    }

    #[test]
    /// Verify that an event can be added to the store correctly and then returned by the store
    ///
    /// 1. Run sqlite migrations
    /// 2. Create DieselAdminServiceEventStore
    /// 3. Create a `messages::AdminServiceEvent`
    /// 4. Add the previously created event to store
    /// 5. List all the events from the store by calling `list_events_since(0)`, which should
    ///    return all events with an ID greater than 0, so all events in the store.
    /// 6. Validate event returned in the list matches the expected values
    fn test_add_list_one_event() {
        let pool = create_connection_pool_and_migrate();

        let store = DieselAdminServiceStore::new(pool);
        let event = create_proposal_submitted_messages_event("test");
        store.add_event(event).expect("Unable to add event");

        let events: Vec<AdminServiceEvent> = store
            .list_events_since(0)
            .expect("Unable to get events from store")
            .collect();
        // Assert only the event added is returned
        assert_eq!(events.len(), 1);
        // Assert the event returned matches the expected values
        assert_eq!(events, vec![create_proposal_submitted_event(1, "test")],);
    }

    #[test]
    /// Verify that events can be added to the store correctly and then returned by the store
    ///
    /// 1. Run sqlite migrations
    /// 2. Create DieselAdminServiceEventStore
    /// 3. Create two `messages::AdminServiceEvent`s
    /// 4. Add the previously created events to store
    /// 5. List all the events from the store by calling `list_events_since(0)`, which should
    ///    return all events with an ID greater than 0, so all events in the store.
    /// 6. Validate the events returned in the list match the expected values
    fn test_list_since_multiple_events() {
        let pool = create_connection_pool_and_migrate();

        let store = DieselAdminServiceStore::new(pool);
        let event_1 = create_proposal_submitted_messages_event("test");
        store.add_event(event_1).expect("Unable to add event");

        let event_2 = create_circuit_ready_messages_event("test");
        store.add_event(event_2).expect("Unable to add event");

        let events: Vec<AdminServiceEvent> = store
            .list_events_since(0)
            .expect("Unable to get events from store")
            .collect();
        // Assert the expected number of events are returned
        assert_eq!(events.len(), 2);
        // Assert the event returned matches the expected values
        assert_eq!(
            events,
            vec![
                create_proposal_submitted_event(1, "test"),
                create_circuit_ready_event(2, "test")
            ],
        );
    }

    #[test]
    /// Verify that events can be added to the store correctly and then returned by the store
    ///
    /// 1. Run sqlite migrations
    /// 2. Create DieselAdminServiceEventStore
    /// 3. Create three `messages::AdminServiceEvent`s
    /// 4. Add the previously created events to store
    /// 5. List the events in the store since the event with an ID of 1
    /// 6. Validate the events returned in the list match the expected values, and the event with
    ///    the ID of 1 is not included
    fn test_list_since() {
        let pool = create_connection_pool_and_migrate();

        let store = DieselAdminServiceStore::new(pool);
        let event_1 = create_proposal_submitted_messages_event("test");
        store.add_event(event_1).expect("Unable to add event");
        let event_2 = create_circuit_ready_messages_event("test");
        store.add_event(event_2).expect("Unable to add event");
        let event_3 = create_proposal_vote_messages_event("test");
        store.add_event(event_3).expect("Unable to add event");

        let events: Vec<AdminServiceEvent> = store
            .list_events_since(1)
            .expect("Unable to get events from store")
            .collect();
        // Assert the expected number of events are returned
        assert_eq!(events.len(), 2);
        // Assert the event returned matches the expected values
        assert_eq!(
            events,
            vec![
                create_circuit_ready_event(2, "test"),
                create_proposal_vote_event(3, "test")
            ],
        );
    }

    #[test]
    /// Verify that events can be added to the store correctly and then returned by the store with
    /// the correct `circuit_management_type`.
    ///
    /// 1. Run sqlite migrations
    /// 2. Create DieselAdminServiceEventStore
    /// 3. Create three `messages::AdminServiceEvent`s
    /// 4. Add the previously created events to store
    /// 5. List the events in the store since the event with an ID of 0 with a
    ///    `circuit_management_type` equal to "not-test".
    /// 6. Validate event returned in the list matches the expected values, including the
    ///    `CircuitProposal` management type.
    fn test_list_one_event_by_management_type() {
        let pool = create_connection_pool_and_migrate();

        let store = DieselAdminServiceStore::new(pool);
        let event = create_proposal_submitted_messages_event("test");
        store.add_event(event).expect("Unable to add event");

        let event_2 = create_circuit_ready_messages_event("not-test");
        store.add_event(event_2).expect("Unable to add event");
        let event_3 = create_proposal_vote_messages_event("test");
        store.add_event(event_3).expect("Unable to add event");

        let events: Vec<AdminServiceEvent> = store
            .list_events_by_management_type_since("not-test".to_string(), 0)
            .expect("Unable to get events from store")
            .collect();
        // Assert one event is returned
        assert_eq!(events.len(), 1);
        // Assert the event returned matches the expected values, with the "not-test" management type
        assert_eq!(events, vec![create_circuit_ready_event(2, "not-test")],);
    }

    #[test]
    /// Verify that events can be added to the store correctly and then returned by the store with
    /// the correct `circuit_management_type`.
    ///
    /// 1. Run sqlite migrations
    /// 2. Create DieselAdminServiceEventStore
    /// 3. Create three `messages::AdminServiceEvent`s
    /// 4. Add the previously created events to store
    /// 5. List the events in the store since the event with an ID of 1 with a
    ///    `circuit_management_type` equal to "not-test".
    /// 6. Validate event returned in the list matches the expected values, including verifying the
    ///    `CircuitProposal`'s `circuit_management_type` and the event ID is not equal or less than
    ///    2.
    fn test_list_event_by_management_type_since() {
        let pool = create_connection_pool_and_migrate();

        let store = DieselAdminServiceStore::new(pool);
        let event = create_proposal_submitted_messages_event("test");
        store.add_event(event).expect("Unable to add event");
        let event_2 = create_circuit_ready_messages_event("not-test");
        store.add_event(event_2).expect("Unable to add event");
        let event_3 = create_proposal_vote_messages_event("test");
        store.add_event(event_3).expect("Unable to add event");

        let events: Vec<AdminServiceEvent> = store
            .list_events_by_management_type_since("not-test".to_string(), 1)
            .expect("Unable to get events from store")
            .collect();
        // Assert one event is returned
        assert_eq!(events.len(), 1);
        // Assert the event returned matches the expected values, with the "not-test" management type
        assert_eq!(events, vec![create_circuit_ready_event(2, "not-test")],);
    }

    #[test]
    /// Verify that events can be added to the store correctly and then returned by the store with
    /// the correct `circuit_management_type`.
    ///
    /// 1. Run sqlite migrations
    /// 2. Create DieselAdminServiceEventStore
    /// 3. Create three `messages::AdminServiceEvent`s
    /// 4. Add the previously created events to store
    /// 5. List the events in the store since the event with an ID of 0 with a
    ///    `circuit_management_type` equal to "test".
    /// 6. Validate the events returned in the list match the expected values, including the
    ///    `CircuitProposal`'s `circuit_management_type`.
    fn test_list_multiple_events_by_management_type() {
        let pool = create_connection_pool_and_migrate();

        let store = DieselAdminServiceStore::new(pool);
        let event = create_proposal_submitted_messages_event("test");
        store.add_event(event).expect("Unable to add event");
        let event_2 = create_circuit_ready_messages_event("not-test");
        store.add_event(event_2).expect("Unable to add event");
        let event_3 = create_proposal_vote_messages_event("test");
        store.add_event(event_3).expect("Unable to add event");

        let events: Vec<AdminServiceEvent> = store
            .list_events_by_management_type_since("test".to_string(), 0)
            .expect("Unable to get events from store")
            .collect();
        // Assert the expected number of events is returned
        assert_eq!(events.len(), 2);
        // Assert the event returned matches the expected values, with the "test" management type
        assert_eq!(
            events,
            vec![
                create_proposal_submitted_event(1, "test"),
                create_proposal_vote_event(3, "test")
            ],
        );
    }

    /// Creates a connection pool for an in-memory SQLite database with only a single connection
    /// available. Each connection is backed by a different in-memory SQLite database, so limiting
    /// the pool to a single connection ensures that the same DB is used for all operations.
    fn create_connection_pool_and_migrate() -> Pool<ConnectionManager<SqliteConnection>> {
        let connection_manager = ConnectionManager::<SqliteConnection>::new(":memory:");
        let pool = Pool::builder()
            .max_size(1)
            .build(connection_manager)
            .expect("Failed to build connection pool");

        run_sqlite_migrations(&*pool.get().expect("Failed to get connection for migrations"))
            .expect("Failed to run migrations");

        pool
    }

    fn create_proposal() -> CircuitProposal {
        CircuitProposalBuilder::default()
            .with_proposal_type(&ProposalType::Create)
            .with_circuit_id("WBKLF-BBBBB")
            .with_circuit_hash(
                "7ddc426972710adc0b2ecd49e89a9dd805fb9206bf516079724c887bedbcdf1d")
            .with_circuit(
                &ProposedCircuitBuilder::default()
                    .with_circuit_id("WBKLF-BBBBB")
                    .with_roster(&vec![
                        ProposedServiceBuilder::default()
                            .with_service_id("a000")
                            .with_service_type("scabbard")
                            .with_node_id(&"acme-node-000")
                            .with_arguments(&vec![
                                ("peer_services".into(), "[\"a001\"]".into()),
                                ("admin_keys".into(),
                               "[\"035724d11cae47c8907f8bfdf510488f49df8494ff81b63825bad923733c4ac550\"]".into())
                            ])
                            .build().expect("Unable to build service"),
                        ProposedServiceBuilder::default()
                            .with_service_id("a001")
                            .with_service_type("scabbard")
                            .with_node_id(&"bubba-node-000")
                            .with_arguments(&vec![
                                ("peer_services".into(), "[\"a000\"]".into()),
                                ("admin_keys".into(),
                               "[\"035724d11cae47c8907f8bfdf510488f49df8494ff81b63825bad923733c4ac550\"]".into())
                            ])
                            .build().expect("Unable to build service")
                        ])

                    .with_members(
                        &vec![
                        ProposedNodeBuilder::default()
                            .with_node_id("bubba-node-000".into())
                            .with_endpoints(
                                &vec!["tcps://splinterd-node-bubba:8044".into(),
                                      "tcps://splinterd-node-bubba-2:8044".into()])
                            .build().expect("Unable to build node"),
                        ProposedNodeBuilder::default()
                            .with_node_id("acme-node-000".into())
                            .with_endpoints(&vec!["tcps://splinterd-node-acme:8044".into()])
                            .build().expect("Unable to build node"),
                        ]
                    )
                    .with_circuit_version(3)
                    .with_application_metadata(b"test")
                    .with_comments("This is a test")
                    .with_circuit_management_type("gameroom")
                    .with_display_name("test_display")
                    .build()
                    .expect("Unable to build circuit")
            )
            .with_requester(
                &PublicKey::from_bytes(parse_hex(
                    "0283a14e0a17cb7f665311e9b5560f4cde2b502f17e2d03223e15d90d9318d7482").unwrap()))
            .with_requester_node_id("acme-node-000")
            .with_votes(&vec![VoteRecordBuilder::new()
                .with_public_key(
                    &PublicKey::from_bytes(parse_hex(
                        "035724d11cae47c8907f8bfdf510488f49df8494ff81b63825bad923733c4ac550",
                    )
                    .unwrap()),
                )
                .with_vote(&Vote::Accept)
                .with_voter_node_id("bubba-node-000")
                .build()
                .expect("Unable to build vote record"),
                VoteRecordBuilder::new()
                    .with_public_key(
                        &PublicKey::from_bytes(parse_hex(
                            "035724d11cae47c8907f8bfdf510488f49df8494ff81b63825bad923733c4ac550",
                        )
                        .unwrap()),
                    )
                    .with_vote(&Vote::Accept)
                    .with_voter_node_id("bubba-node-002")
                    .build()
                    .expect("Unable to build vote record")]
            )
            .build().expect("Unable to build proposals")
    }

    fn create_extra_proposal() -> CircuitProposal {
        CircuitProposalBuilder::default()
            .with_proposal_type(&ProposalType::Create)
            .with_circuit_id("WBKLF-AAAAA")
            .with_circuit_hash(
                "7ddc426972710adc0b2ecd49e89a9dd805fb9206bf516079724c887bedbcdf1d")
            .with_circuit(
                &ProposedCircuitBuilder::default()
                    .with_circuit_id("WBKLF-AAAAA")
                    .with_roster(&vec![
                        ProposedServiceBuilder::default()
                            .with_service_id("a000")
                            .with_service_type("scabbard")
                            .with_node_id(&"acme-node-000")
                            .with_arguments(&vec![
                                ("peer_services".into(), "[\"a001\"]".into()),
                                ("admin_keys".into(),
                               "[\"035724d11cae47c8907f8bfdf510488f49df8494ff81b63825bad923733c4ac550\"]".into())
                            ])
                            .build().expect("Unable to build service"),
                        ProposedServiceBuilder::default()
                            .with_service_id("a001")
                            .with_service_type("scabbard")
                            .with_node_id(&"gumbo-node-000")
                            .with_arguments(&vec![
                                ("peer_services".into(), "[\"a000\"]".into()),
                                ("admin_keys".into(),
                               "[\"035724d11cae47c8907f8bfdf510488f49df8494ff81b63825bad923733c4ac550\"]".into())
                            ])
                            .build().expect("Unable to build service")
                        ])

                    .with_members(
                        &vec![
                        ProposedNodeBuilder::default()
                            .with_node_id("gumbo-node-000".into())
                            .with_endpoints(&vec!["tcps://splinterd-node-gumbo:8044".into()])
                            .build().expect("Unable to build node"),
                        ProposedNodeBuilder::default()
                            .with_node_id("acme-node-000".into())
                            .with_endpoints(&vec!["tcps://splinterd-node-acme:8044".into()])
                            .build().expect("Unable to build node"),
                        ]
                    )
                    .with_circuit_management_type("gameroom")
                    .with_circuit_status(&CircuitStatus::Active)
                    .build().expect("Unable to build circuit")
            )
            .with_requester(
                &PublicKey::from_bytes(parse_hex(
                    "0283a14e0a17cb7f665311e9b5560f4cde2b502f17e2d03223e15d90d9318d7482").unwrap()))
            .with_requester_node_id("acme-node-000")
            .build().expect("Unable to build proposals")
    }

    fn create_circuit(circuit_id: &str, status: CircuitStatus) -> Circuit {
        let nodes = create_nodes();

        CircuitBuilder::default()
            .with_circuit_id(circuit_id)
            .with_roster(&vec![
                ServiceBuilder::default()
                    .with_service_id("a000")
                    .with_service_type("scabbard")
                    .with_node_id("acme-node-000")
                    .with_arguments(&vec![
                        ("peer_services".into(), "[\"a001\"]".into()),
                        ("admin_keys".into(),
                       "[\"035724d11cae47c8907f8bfdf510488f49df8494ff81b63825bad923733c4ac550\"]".into())
                    ])
                    .build()
                    .expect("Unable to build service"),
                ServiceBuilder::default()
                    .with_service_id("a001")
                    .with_service_type("scabbard")
                    .with_node_id("bubba-node-000")
                    .with_arguments(&vec![
                        ("peer_services".into(), "[\"a000\"]".into()),
                        ("admin_keys".into(),
                       "[\"035724d11cae47c8907f8bfdf510488f49df8494ff81b63825bad923733c4ac550\"]".into())
                    ])
                    .build()
                    .expect("Unable to build service"),
            ])
            .with_members(&nodes)
            .with_circuit_management_type("gameroom")
            .with_display_name("test_display")
            .with_circuit_version(3)
            .with_circuit_status(&status)
            .build()
            .expect("Unable to build circuit")
    }

    fn create_circuit_from_proposal(circuit_id: &str, status: CircuitStatus) -> Circuit {
        CircuitBuilder::default()
            .with_circuit_id(circuit_id)
            .with_roster(&vec![
                ServiceBuilder::default()
                    .with_service_id("a000")
                    .with_service_type("scabbard")
                    .with_node_id("acme-node-000")
                    .with_arguments(&vec![
                        ("peer_services".into(), "[\"a001\"]".into()),
                        ("admin_keys".into(),
                       "[\"035724d11cae47c8907f8bfdf510488f49df8494ff81b63825bad923733c4ac550\"]".into())
                    ])
                    .build()
                    .expect("Unable to build service"),
                ServiceBuilder::default()
                    .with_service_id("a001")
                    .with_service_type("scabbard")
                    .with_node_id("bubba-node-000")
                    .with_arguments(&vec![
                        ("peer_services".into(), "[\"a000\"]".into()),
                        ("admin_keys".into(),
                       "[\"035724d11cae47c8907f8bfdf510488f49df8494ff81b63825bad923733c4ac550\"]".into())
                    ])
                    .build()
                    .expect("Unable to build service"),
            ])
            .with_members(
                &vec![
                CircuitNodeBuilder::default()
                    .with_node_id("bubba-node-000".into())
                    .with_endpoints(
                        &vec!["tcps://splinterd-node-bubba:8044".into(),
                              "tcps://splinterd-node-bubba-2:8044".into()])
                    .build().expect("Unable to build node"),
                CircuitNodeBuilder::default()
                    .with_node_id("acme-node-000".into())
                    .with_endpoints(&vec!["tcps://splinterd-node-acme:8044".into()])
                    .build().expect("Unable to build node"),
                ]
            )
            .with_circuit_management_type("gameroom")
            .with_display_name("test_display")
            .with_circuit_version(3)
            .with_circuit_status(&status)
            .build()
            .expect("Unable to build circuit")
    }

    fn create_extra_circuit(circuit_id: &str) -> Circuit {
        let nodes = create_extra_nodes();
        CircuitBuilder::default()
            .with_circuit_id(circuit_id)
            .with_roster(&vec![
                ServiceBuilder::default()
                    .with_service_id("a000")
                    .with_service_type("scabbard")
                    .with_node_id("acme-node-000")
                    .with_arguments(&vec![
                        ("admin_keys".into(),
                       "[\"035724d11cae47c8907f8bfdf510488f49df8494ff81b63825bad923733c4ac550\"]"
                            .into()),
                       ("peer_services".into(), "[\"a001\"]".into()),
                    ])
                    .build()
                    .expect("Unable to build service"),
                ServiceBuilder::default()
                    .with_service_id("a001")
                    .with_service_type("scabbard")
                    .with_node_id("gumbo-node-000")
                    .with_arguments(&vec![(
                        "admin_keys".into(),
                        "[\"035724d11cae47c8907f8bfdf510488f49df8494ff81b63825bad923733c4ac550\"]"
                            .into()
                    ),(
                        "peer_services".into(), "[\"a000\"]".into()
                    )])
                    .build()
                    .expect("Unable to build service"),
            ])
            .with_members(&nodes)
            .with_circuit_management_type("other")
            .build()
            .expect("Unable to build circuit")
    }

    // Creates a admin store `CircuitProposal` that is equivalent to the type of `CircuitProposal`
    // created from an admin::messages::CircuitProposal. Specifically, the `circuit_version`
    // is set to 1.
    fn create_messages_proposal(management_type: &str) -> CircuitProposal {
        CircuitProposalBuilder::default()
            .with_proposal_type(&ProposalType::Create)
            .with_circuit_id("WBKLF-BBBBB")
            .with_circuit_hash(
                "7ddc426972710adc0b2ecd49e89a9dd805fb9206bf516079724c887bedbcdf1d")
            .with_circuit(
                &ProposedCircuitBuilder::default()
                    .with_circuit_id("WBKLF-BBBBB")
                    .with_roster(&vec![
                        ProposedServiceBuilder::default()
                            .with_service_id("a000")
                            .with_service_type("scabbard")
                            .with_node_id(&"acme-node-000")
                            .with_arguments(&vec![
                                ("peer_services".into(), "[\"a001\"]".into()),
                                ("admin_keys".into(),
                               "[\"035724d11cae47c8907f8bfdf510488f49df8494ff81b63825bad923733c4ac550\"]".into())
                            ])
                            .build().expect("Unable to build service"),
                        ProposedServiceBuilder::default()
                            .with_service_id("a001")
                            .with_service_type("scabbard")
                            .with_node_id(&"bubba-node-000")
                            .with_arguments(&vec![
                                ("peer_services".into(), "[\"a000\"]".into()),
                                ("admin_keys".into(),
                               "[\"035724d11cae47c8907f8bfdf510488f49df8494ff81b63825bad923733c4ac550\"]".into())
                            ])
                            .build().expect("Unable to build service")
                        ])

                    .with_members(
                        &vec![
                        ProposedNodeBuilder::default()
                            .with_node_id("bubba-node-000".into())
                            .with_endpoints(
                                &vec!["tcps://splinterd-node-bubba:8044".into(),
                                      "tcps://splinterd-node-bubba-2:8044".into()])
                            .build().expect("Unable to build node"),
                        ProposedNodeBuilder::default()
                            .with_node_id("acme-node-000".into())
                            .with_endpoints(&vec!["tcps://splinterd-node-acme:8044".into()])
                            .build().expect("Unable to build node"),
                        ]
                    )
                    .with_circuit_version(1)
                    .with_application_metadata(b"test")
                    .with_comments("This is a test")
                    .with_circuit_management_type(management_type)
                    .with_display_name("test_display")
                    .build()
                    .expect("Unable to build circuit")
            )
            .with_requester(
                &PublicKey::from_bytes(parse_hex(
                    "0283a14e0a17cb7f665311e9b5560f4cde2b502f17e2d03223e15d90d9318d7482").unwrap()))
            .with_requester_node_id("acme-node-000")
            .with_votes(&vec![VoteRecordBuilder::new()
                .with_public_key(
                    &PublicKey::from_bytes(parse_hex(
                        "035724d11cae47c8907f8bfdf510488f49df8494ff81b63825bad923733c4ac550",
                    )
                    .unwrap()),
                )
                .with_vote(&Vote::Accept)
                .with_voter_node_id("bubba-node-000")
                .build()
                .expect("Unable to build vote record"),
                VoteRecordBuilder::new()
                    .with_public_key(
                        &PublicKey::from_bytes(parse_hex(
                            "035724d11cae47c8907f8bfdf510488f49df8494ff81b63825bad923733c4ac550",
                        )
                        .unwrap()),
                    )
                    .with_vote(&Vote::Accept)
                    .with_voter_node_id("bubba-node-002")
                    .build()
                    .expect("Unable to build vote record")]
            )
            .build().expect("Unable to build proposals")
    }

    fn create_nodes() -> Vec<CircuitNode> {
        vec![
            CircuitNodeBuilder::default()
                .with_node_id("bubba-node-000".into())
                .with_endpoints(&vec!["tcps://splinterd-node-bubba:8044".into()])
                .build()
                .expect("Unable to build node"),
            CircuitNodeBuilder::default()
                .with_node_id("acme-node-000".into())
                .with_endpoints(&vec!["tcps://splinterd-node-acme:8044".into()])
                .build()
                .expect("Unable to build node"),
        ]
    }

    fn create_extra_nodes() -> Vec<CircuitNode> {
        vec![
            CircuitNodeBuilder::default()
                .with_node_id("gumbo-node-000".into())
                .with_endpoints(&vec!["tcps://splinterd-node-gumbo:8044".into()])
                .build()
                .expect("Unable to build node"),
            CircuitNodeBuilder::default()
                .with_node_id("acme-node-000".into())
                .with_endpoints(&vec!["tcps://splinterd-node-acme:8044".into()])
                .build()
                .expect("Unable to build node"),
        ]
    }

    fn create_proposal_submitted_event(event_id: i64, management_type: &str) -> AdminServiceEvent {
        AdminServiceEventBuilder::new()
            .with_event_id(event_id)
            .with_event_type(&EventType::ProposalSubmitted)
            .with_proposal(&create_messages_proposal(management_type))
            .build()
            .expect("Unable to build AdminServiceEvent")
    }

    fn create_proposal_submitted_messages_event(
        management_type: &str,
    ) -> messages::AdminServiceEvent {
        messages::AdminServiceEvent::ProposalSubmitted(messages::CircuitProposal::from(
            create_messages_proposal(management_type),
        ))
    }

    fn create_circuit_ready_event(event_id: i64, management_type: &str) -> AdminServiceEvent {
        AdminServiceEventBuilder::new()
            .with_event_id(event_id)
            .with_event_type(&EventType::CircuitReady)
            .with_proposal(&create_messages_proposal(management_type))
            .build()
            .expect("Unable to build AdminServiceEvent")
    }

    fn create_circuit_ready_messages_event(management_type: &str) -> messages::AdminServiceEvent {
        messages::AdminServiceEvent::CircuitReady(messages::CircuitProposal::from(
            create_messages_proposal(management_type),
        ))
    }

    fn create_proposal_vote_event(event_id: i64, management_type: &str) -> AdminServiceEvent {
        let requester =
            &parse_hex("0283a14e0a17cb7f665311e9b5560f4cde2b502f17e2d03223e15d90d9318d7482")
                .unwrap();
        AdminServiceEventBuilder::new()
            .with_event_id(event_id)
            .with_event_type(&EventType::ProposalVote {
                requester: requester.to_vec(),
            })
            .with_proposal(&create_messages_proposal(management_type))
            .build()
            .expect("Unable to build AdminServiceEvent")
    }

    fn create_proposal_vote_messages_event(management_type: &str) -> messages::AdminServiceEvent {
        let requester =
            &parse_hex("0283a14e0a17cb7f665311e9b5560f4cde2b502f17e2d03223e15d90d9318d7482")
                .unwrap();

        messages::AdminServiceEvent::ProposalVote((
            messages::CircuitProposal::from(create_messages_proposal(management_type)),
            requester.to_vec(),
        ))
    }
}

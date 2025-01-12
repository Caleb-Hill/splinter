/*
 * Copyright 2018-2022 Cargill Incorporated
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 * -----------------------------------------------------------------------------
 */

mod error;
pub use error::AppAuthHandlerError;
pub mod sabre;
mod state_delta;

use std::fmt::Write;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use diesel::connection::Connection;
use gameroom_database::{
    helpers,
    models::{
        ActiveGameroom, Gameroom, GameroomProposal, NewGameroomMember, NewGameroomProposal,
        NewGameroomService, NewProposalVoteRecord,
    },
    ConnectionPool,
};
use scabbard::{protocol::SCABBARD_PROTOCOL_VERSION, service::StateChangeEvent};
use splinter::{
    admin::messages::v1::{
        AdminServiceEvent, CircuitProposal, CreateCircuit, SplinterNode, SplinterService,
    },
    events::{Igniter, ParseBytes, ParseError, WebSocketClient, WebSocketError, WsResponse},
};
use state_delta::XoStateDeltaProcessor;

use crate::application_metadata::ApplicationMetadata;

use self::sabre::setup_xo;

/// default value if the client should attempt to reconnet if ws connection is lost
const RECONNECT: bool = true;

/// default limit for number of consecutives failed reconnection attempts
const RECONNECT_LIMIT: u64 = 10;

/// default timeout in seconds if no message is received from server
const CONNECTION_TIMEOUT: u64 = 60;

// The admin protocol version supported by the gameroom  app auth handler
const GAMEROOM_ADMIN_PROTOCOL_VERSION: &str = "1";

#[derive(Deserialize, Debug, Clone)]
struct Event {
    timestamp: u64,

    #[serde(flatten)]
    admin_event: AdminServiceEvent,
}

impl ParseBytes<Event> for Event {
    fn from_bytes(bytes: &[u8]) -> Result<Self, ParseError> {
        serde_json::from_slice(bytes).map_err(|err| ParseError::MalformedMessage(Box::new(err)))
    }
}

pub fn run(
    splinterd_url: String,
    authorization: String,
    node_id: String,
    db_conn: ConnectionPool,
    private_key: String,
    igniter: Igniter,
) -> Result<(), AppAuthHandlerError> {
    let pool = db_conn.get()?;
    let registration_route = helpers::get_last_updated_proposal_time(&pool)?
        .map(|time| {
            format!(
                "{}/ws/admin/register/gameroom?last={}",
                splinterd_url,
                time.duration_since(SystemTime::UNIX_EPOCH)
                    .map(|duration| duration.as_millis())
                    .unwrap_or(0)
            )
        })
        .unwrap_or_else(|| format!("{}/ws/admin/register/gameroom", splinterd_url));

    let ws_url = splinterd_url.clone();
    let ws_authorization = authorization.clone();
    let ws_node_id = node_id.clone();
    let ws_db_conn = db_conn.clone();
    let mut ws = WebSocketClient::new(&registration_route, &authorization, move |ctx, event| {
        if let Err(err) = process_admin_event(
            event,
            &ws_db_conn,
            &ws_node_id,
            &private_key,
            &ws_url,
            &ws_authorization,
            ctx.igniter(),
        ) {
            error!("Failed to process admin event: {}", err);
        }
        WsResponse::Empty
    });

    let on_open_db_conn = db_conn.clone();
    let on_open_igniter = igniter.clone();
    let on_open_url = splinterd_url.clone();
    let on_open_authorization = authorization.clone();
    ws.on_open(move |_| {
        let conn = match on_open_db_conn.get() {
            Ok(conn) => conn,
            Err(err) => {
                error!("Failed to create database connection: {}", err);
                return WsResponse::Empty;
            }
        };

        let gamerooms = match helpers::fetch_active_gamerooms(&conn, &node_id) {
            Ok(gamerooms) => gamerooms,
            Err(err) => {
                error!("Failed to retrieve active gamerooms: {}", err);
                return WsResponse::Empty;
            }
        };

        for gameroom in gamerooms.iter() {
            let ws = resubscribe(
                &on_open_url,
                &on_open_authorization,
                gameroom,
                &on_open_db_conn,
            );
            if let Err(err) = on_open_igniter.start_ws(&ws) {
                error!("Failed to resubscribe to active gameroom: {}", err);
            }
        }

        WsResponse::Empty
    });

    ws.header(
        "SplinterProtocolVersion",
        GAMEROOM_ADMIN_PROTOCOL_VERSION.to_string(),
    );

    ws.set_reconnect(RECONNECT);
    ws.set_reconnect_limit(RECONNECT_LIMIT);
    ws.set_timeout(CONNECTION_TIMEOUT);

    let on_reconnect_url = splinterd_url.clone();
    ws.on_reconnect(move |ws| {
        debug!("Authorization handler attempting reconnect");
        match db_conn.get() {
            Ok(conn) => {
                let url = helpers::get_last_updated_proposal_time(&conn)
                    .unwrap_or_else(|err| {
                        warn!("Proposal time could not be retrieved {}", err);
                        None
                    })
                    .map(|time| {
                        format!(
                            "{}/ws/admin/register/gameroom?last={}",
                            on_reconnect_url,
                            time.duration_since(SystemTime::UNIX_EPOCH)
                                .map(|duration| duration.as_millis())
                                .unwrap_or(0)
                        )
                    })
                    .unwrap_or_else(|| format!("{}/ws/admin/register/gameroom", on_reconnect_url));

                ws.set_url(&url);
            }
            Err(err) => {
                error!("Failed to retrieve database connection: {}", err);
            }
        }
    });

    let on_error_url = splinterd_url.clone();
    ws.on_error(move |err, ctx| {
        error!("An error occured while listening for admin events {}", err);
        match err {
            WebSocketError::ParserError { .. } => {
                error!("Protocol error, closing connection");
                Ok(())
            }
            WebSocketError::ReconnectError(_) => {
                error!("Failed to reconnect. Closing WebSocket.");
                Ok(())
            }
            _ => {
                let now = Instant::now();

                debug!("Checking for splinterd server");
                while now.elapsed() <= Duration::from_secs(30) {
                    match reqwest::blocking::Client::new()
                        .get(&format!("{}/ws/admin/register/gameroom", on_error_url))
                        .header("Authorization", &authorization)
                        .send()
                    {
                        Ok(res) => {
                            if res.status().is_success() {
                                debug!(
                                    "splinterd server {} available reconnecting..",
                                    splinterd_url
                                );
                                return ctx.start_ws();
                            }
                        }
                        Err(err) => {
                            warn!("Error occurred trying to detect splinterd server: {}", err);
                            break;
                        }
                    }

                    thread::sleep(Duration::from_secs(1));
                }
                Ok(())
            }
        }
    });

    igniter.start_ws(&ws).map_err(AppAuthHandlerError::from)
}

fn process_admin_event(
    event: Event,
    pool: &ConnectionPool,
    node_id: &str,
    private_key: &str,
    url: &str,
    authorization: &str,
    igniter: Igniter,
) -> Result<(), AppAuthHandlerError> {
    debug!("Received the event at {}", event.timestamp);
    let time: SystemTime = SystemTime::UNIX_EPOCH + Duration::from_millis(event.timestamp);
    match event.admin_event {
        AdminServiceEvent::ProposalSubmitted(msg_proposal) => {
            // convert requester public key to hex
            let requester = to_hex(&msg_proposal.requester);
            let proposal = parse_proposal(&msg_proposal, time, requester);

            let gameroom = parse_gameroom(&msg_proposal.circuit, time)?;

            let services = parse_splinter_services(
                &msg_proposal.circuit_id,
                &msg_proposal.circuit.roster,
                time,
            );

            let nodes = parse_splinter_nodes(
                &msg_proposal.circuit_id,
                &msg_proposal.circuit.members,
                time,
            );

            let conn = &*pool.get()?;

            // insert proposal information in database tables in a single transaction
            conn.transaction::<_, _, _>(|| {
                let notification = helpers::create_new_notification(
                    "gameroom_proposal",
                    &proposal.requester,
                    &proposal.requester_node_id,
                    &proposal.circuit_id,
                );
                helpers::insert_gameroom_notification(conn, &[notification])?;

                helpers::insert_gameroom(conn, gameroom)?;
                helpers::insert_gameroom_proposal(conn, proposal)?;
                helpers::insert_gameroom_services(conn, &services)?;
                helpers::insert_gameroom_members(conn, &nodes)?;

                debug!("Inserted new proposal into database");
                Ok(())
            })
        }
        AdminServiceEvent::ProposalVote((msg_proposal, signer_public_key)) => {
            let proposal = get_pending_proposal_with_circuit_id(pool, &msg_proposal.circuit_id)?;
            let vote = msg_proposal
                .votes
                .iter()
                .find(|vote| vote.public_key == signer_public_key)
                .ok_or_else(|| {
                    AppAuthHandlerError::InvalidMessage("Missing vote from signer".to_string())
                })?;
            let vote = NewProposalVoteRecord {
                proposal_id: proposal.id,
                voter_public_key: to_hex(&signer_public_key),
                voter_node_id: vote.voter_node_id.to_string(),
                vote: "Accept".to_string(),
                created_time: time,
            };
            let conn = &*pool.get()?;

            // insert vote and update proposal in a single database transaction
            conn.transaction::<_, _, _>(|| {
                let notification = helpers::create_new_notification(
                    "proposal_vote_record",
                    &vote.voter_public_key,
                    &vote.voter_node_id,
                    &msg_proposal.circuit_id,
                );
                helpers::insert_gameroom_notification(conn, &[notification])?;
                helpers::update_gameroom_proposal_status(conn, proposal.id, &time, "Pending")?;
                helpers::insert_proposal_vote_record(conn, &[vote])?;

                debug!("Inserted new vote into database");
                Ok(())
            })
        }
        AdminServiceEvent::ProposalAccepted((msg_proposal, signer_public_key)) => {
            let proposal = get_pending_proposal_with_circuit_id(pool, &msg_proposal.circuit_id)?;
            let vote = msg_proposal
                .votes
                .iter()
                .find(|vote| vote.public_key == signer_public_key)
                .ok_or_else(|| {
                    AppAuthHandlerError::InvalidMessage("Missing vote from signer".to_string())
                })?;

            let vote = NewProposalVoteRecord {
                proposal_id: proposal.id,
                voter_public_key: to_hex(&signer_public_key),
                voter_node_id: vote.voter_node_id.to_string(),
                vote: "Accept".to_string(),
                created_time: time,
            };
            let conn = &*pool.get()?;

            // insert vote and update proposal in a single database transaction
            conn.transaction::<_, _, _>(|| {
                let notification = helpers::create_new_notification(
                    "proposal_accepted",
                    &vote.voter_public_key,
                    &vote.voter_node_id,
                    &msg_proposal.circuit_id,
                );
                helpers::insert_gameroom_notification(conn, &[notification])?;
                helpers::update_gameroom_proposal_status(conn, proposal.id, &time, "Accepted")?;
                helpers::update_gameroom_status(conn, &msg_proposal.circuit_id, &time, "Accepted")?;
                helpers::update_gameroom_member_status(
                    conn,
                    &msg_proposal.circuit_id,
                    &time,
                    "Pending",
                    "Accepted",
                )?;
                helpers::update_gameroom_service_status(
                    conn,
                    &msg_proposal.circuit_id,
                    &time,
                    "Pending",
                    "Accepted",
                )?;

                helpers::insert_proposal_vote_record(conn, &[vote])?;

                debug!("Updated proposal to status 'Accepted'");
                Ok(())
            })
        }
        AdminServiceEvent::ProposalRejected((msg_proposal, signer_public_key)) => {
            let proposal = get_pending_proposal_with_circuit_id(pool, &msg_proposal.circuit_id)?;
            let vote = msg_proposal
                .votes
                .iter()
                .find(|vote| vote.public_key == signer_public_key)
                .ok_or_else(|| {
                    AppAuthHandlerError::InvalidMessage("Missing vote from signer".to_string())
                })?;

            let vote = NewProposalVoteRecord {
                proposal_id: proposal.id,
                voter_public_key: to_hex(&signer_public_key),
                voter_node_id: vote.voter_node_id.to_string(),
                vote: "Reject".to_string(),
                created_time: time,
            };
            let conn = &*pool.get()?;

            // insert vote and update proposal in a single database transaction
            conn.transaction::<_, _, _>(|| {
                let notification = helpers::create_new_notification(
                    "proposal_rejected",
                    &vote.voter_public_key,
                    &vote.voter_node_id,
                    &msg_proposal.circuit_id,
                );
                helpers::insert_gameroom_notification(conn, &[notification])?;
                helpers::update_gameroom_proposal_status(conn, proposal.id, &time, "Rejected")?;
                helpers::update_gameroom_status(conn, &msg_proposal.circuit_id, &time, "Rejected")?;
                helpers::update_gameroom_member_status(
                    conn,
                    &msg_proposal.circuit_id,
                    &time,
                    "Pending",
                    "Rejected",
                )?;
                helpers::update_gameroom_service_status(
                    conn,
                    &msg_proposal.circuit_id,
                    &time,
                    "Pending",
                    "Rejected",
                )?;
                helpers::insert_proposal_vote_record(conn, &[vote])?;
                debug!("Updated proposal to status 'Rejected'");
                Ok(())
            })
        }
        AdminServiceEvent::CircuitReady(msg_proposal) => {
            let conn = &*pool.get()?;

            // If the gameroom already exists and is in the ready state, skip
            // processing the event.
            if helpers::gameroom_service_is_active(conn, &msg_proposal.circuit_id)? {
                return Ok(());
            }

            // Now that the circuit is created, submit the Sabre transactions to run xo
            let service_id = match msg_proposal.circuit.roster.iter().find_map(|service| {
                if service.allowed_nodes.contains(&node_id.to_string()) {
                    Some(service.service_id.clone())
                } else {
                    None
                }
            }) {
                Some(id) => id,
                None => {
                    debug!(
                        "New gameroom does not have any services for this node: {}",
                        node_id
                    );
                    return Ok(());
                }
            };
            let scabbard_admin_keys = match serde_json::from_slice::<ApplicationMetadata>(
                msg_proposal.circuit.application_metadata.as_slice(),
            ) {
                Ok(metadata) => metadata.scabbard_admin_keys().to_vec(),
                Err(err) => {
                    return Err(AppAuthHandlerError::InvalidMessage(format!(
                        "unable to parse application metadata: {}",
                        err
                    )))
                }
            };

            let requester = to_hex(&msg_proposal.requester);
            let proposal = parse_proposal(&msg_proposal, time, requester);

            conn.transaction::<_, AppAuthHandlerError, _>(|| {
                let notification = helpers::create_new_notification(
                    "circuit_ready",
                    &proposal.requester,
                    &proposal.requester_node_id,
                    &proposal.circuit_id,
                );
                helpers::insert_gameroom_notification(conn, &[notification])?;
                helpers::update_gameroom_status(conn, &msg_proposal.circuit_id, &time, "Ready")?;
                helpers::update_gameroom_member_status(
                    conn,
                    &msg_proposal.circuit_id,
                    &time,
                    "Accepted",
                    "Ready",
                )?;
                helpers::update_gameroom_service_status(
                    conn,
                    &msg_proposal.circuit_id,
                    &time,
                    "Accepted",
                    "Ready",
                )?;

                debug!("Updated proposal to status 'Ready'");

                Ok(())
            })?;

            let processor = XoStateDeltaProcessor::new(
                &msg_proposal.circuit_id,
                &proposal.requester_node_id,
                &proposal.requester,
                pool,
            )?;

            let mut xo_ws = WebSocketClient::new(
                &format!(
                    "{}/scabbard/{}/{}/ws/subscribe",
                    url, msg_proposal.circuit_id, service_id
                ),
                authorization,
                move |_, event| {
                    if let Err(err) = processor.handle_state_change_event(event) {
                        error!(
                            "An error occurred while handling a state change event: {:?}",
                            err
                        );
                    }
                    WsResponse::Empty
                },
            );

            xo_ws.header(
                "SplinterProtocolVersion",
                SCABBARD_PROTOCOL_VERSION.to_string(),
            );

            let url_to_string = url.to_string();
            let authorization_to_string = authorization.to_string();
            let private_key_to_string = private_key.to_string();
            xo_ws.on_open(move |ctx| {
                debug!("Starting XO State Delta Export");
                let future = match setup_xo(
                    &private_key_to_string,
                    scabbard_admin_keys.clone(),
                    &url_to_string,
                    &authorization_to_string,
                    &msg_proposal.circuit_id.clone(),
                    &service_id.clone(),
                ) {
                    Ok(f) => f,
                    Err(err) => {
                        error!("{}", err);
                        return WsResponse::Close;
                    }
                };

                if let Err(err) = ctx.igniter().send(future) {
                    error!("Failed to setup scabbard: {}", err);
                    WsResponse::Close
                } else {
                    WsResponse::Empty
                }
            });

            xo_ws.on_error(move |err, _| {
                error!(
                    "An error occured while listening for scabbard events {}",
                    err
                );
                Ok(())
            });

            igniter.start_ws(&xo_ws).map_err(AppAuthHandlerError::from)
        }
    }
}

fn resubscribe(
    url: &str,
    authorization: &str,
    gameroom: &ActiveGameroom,
    db_pool: &ConnectionPool,
) -> WebSocketClient<StateChangeEvent> {
    let processor = XoStateDeltaProcessor::new(
        &gameroom.circuit_id,
        &gameroom.requester_node_id,
        &gameroom.requester,
        db_pool,
    );

    let query_string = if gameroom.last_event.is_empty() {
        "".into()
    } else {
        format!("?last_seen_event={}", gameroom.last_event)
    };

    let mut ws = WebSocketClient::new(
        &format!(
            "{}/scabbard/{}/{}/ws/subscribe{}",
            url, gameroom.circuit_id, gameroom.service_id, query_string,
        ),
        authorization,
        move |_, event| {
            match &processor {
                Ok(processor) => {
                    if let Err(err) = processor.handle_state_change_event(event) {
                        error!(
                            "An error occurred while handling a state change event: {:?}",
                            err
                        );
                    }
                }
                Err(err) => error!("Failed to initialize state delta processor: {:?}", err),
            }
            WsResponse::Empty
        },
    );

    ws.header(
        "SplinterProtocolVersion",
        SCABBARD_PROTOCOL_VERSION.to_string(),
    );

    ws.on_error(move |err, _| {
        error!(
            "An error occured while listening for scabbard events {}",
            err
        );
        Ok(())
    });

    ws
}

fn parse_proposal(
    proposal: &CircuitProposal,
    timestamp: SystemTime,
    requester_public_key: String,
) -> NewGameroomProposal {
    NewGameroomProposal {
        proposal_type: format!("{:?}", proposal.proposal_type),
        circuit_id: proposal.circuit_id.clone(),
        circuit_hash: proposal.circuit_hash.to_string(),
        requester: requester_public_key,
        requester_node_id: proposal.requester_node_id.to_string(),
        status: "Pending".to_string(),
        created_time: timestamp,
        updated_time: timestamp,
    }
}

fn parse_gameroom(
    circuit: &CreateCircuit,
    timestamp: SystemTime,
) -> Result<Gameroom, AppAuthHandlerError> {
    let application_metadata = ApplicationMetadata::from_bytes(&circuit.application_metadata)?;

    Ok(Gameroom {
        circuit_id: circuit.circuit_id.clone(),
        authorization_type: format!("{:?}", circuit.authorization_type),
        persistence: format!("{:?}", circuit.persistence),
        durability: format!("{:?}", circuit.durability),
        routes: format!("{:?}", circuit.routes),
        circuit_management_type: circuit.circuit_management_type.clone(),
        alias: application_metadata.alias().to_string(),
        status: "Pending".to_string(),
        created_time: timestamp,
        updated_time: timestamp,
    })
}

fn parse_splinter_services(
    circuit_id: &str,
    splinter_services: &[SplinterService],
    timestamp: SystemTime,
) -> Vec<NewGameroomService> {
    splinter_services
        .iter()
        .map(|service| NewGameroomService {
            circuit_id: circuit_id.to_string(),
            service_id: service.service_id.to_string(),
            service_type: service.service_type.to_string(),
            allowed_nodes: service.allowed_nodes.clone(),
            arguments: service
                .arguments
                .clone()
                .iter()
                .map(|(key, value)| {
                    json!({
                        "key": key,
                        "value": value
                    })
                })
                .collect(),
            status: "Pending".to_string(),
            last_event: "".to_string(),
            created_time: timestamp,
            updated_time: timestamp,
        })
        .collect()
}

fn parse_splinter_nodes(
    circuit_id: &str,
    splinter_nodes: &[SplinterNode],
    timestamp: SystemTime,
) -> Vec<NewGameroomMember> {
    splinter_nodes
        .iter()
        .map(|node| NewGameroomMember {
            circuit_id: circuit_id.to_string(),
            node_id: node.node_id.to_string(),
            endpoints: node.endpoints.to_vec(),
            status: "Pending".to_string(),
            created_time: timestamp,
            updated_time: timestamp,
        })
        .collect()
}

fn get_pending_proposal_with_circuit_id(
    pool: &ConnectionPool,
    circuit_id: &str,
) -> Result<GameroomProposal, AppAuthHandlerError> {
    helpers::fetch_gameroom_proposal_with_status(&*pool.get()?, circuit_id, "Pending")?.ok_or_else(
        || {
            AppAuthHandlerError::Database(format!(
                "Could not find open proposal for circuit: {}",
                circuit_id
            ))
        },
    )
}

pub fn to_hex(bytes: &[u8]) -> String {
    let mut buf = String::new();
    for b in bytes {
        write!(&mut buf, "{:02x}", b).expect("Unable to write to string");
    }

    buf
}

#[cfg(all(feature = "test-authorization-handler", test))]
mod test {
    use super::*;
    use splinter::events::Reactor;

    use diesel::{dsl::insert_into, prelude::*, RunQueryDsl};
    use gameroom_database::models::{
        GameroomMember, GameroomNotification, GameroomService, NewGameroomNotification,
        ProposalVoteRecord,
    };

    use splinter::admin::messages::v1::{
        AuthorizationType, CreateCircuit, DurabilityType, PersistenceType, ProposalType, RouteType,
        Vote, VoteRecord,
    };

    static DATABASE_URL: &str = "postgres://gameroom_test:gameroom_test@db-test:5432/gameroom_test";

    #[test]
    /// Tests if when receiving an admin message to CreateProposal the gameroom_proposal
    /// table is updated as expected
    fn test_process_proposal_submitted_message_update_proposal_table() {
        let reactor = Reactor::new();
        let pool: ConnectionPool = gameroom_database::create_connection_pool(DATABASE_URL)
            .expect("Failed to get database connection pool");

        clear_gameroom_table(&pool);
        clear_gameroom_notification_table(&pool);

        let message = get_submit_proposal_msg("01234-ABCDE");
        process_admin_event(message, &pool, "", "", "", "", reactor.igniter())
            .expect("Error processing message");

        let proposals = query_proposals_table(&pool);

        assert_eq!(proposals.len(), 1);

        let proposal = &proposals[0];
        let expected_proposal = get_gameroom_proposal("01234-ABCDE", SystemTime::now());

        assert_eq!(proposal.proposal_type, expected_proposal.proposal_type);
        assert_eq!(proposal.circuit_id, expected_proposal.circuit_id);
        assert_eq!(proposal.circuit_hash, expected_proposal.circuit_hash);
        assert_eq!(proposal.requester, expected_proposal.requester);
        assert_eq!(proposal.status, expected_proposal.status);
    }

    #[test]
    /// Tests if when receiving an admin message to CreateProposal the gameroom
    /// table is updated as expected
    fn test_process_proposal_submitted_message_update_gameroom_table() {
        let reactor = Reactor::new();

        let pool: ConnectionPool = gameroom_database::create_connection_pool(DATABASE_URL)
            .expect("Failed to get database connection pool");

        clear_gameroom_table(&pool);
        clear_gameroom_notification_table(&pool);

        let message = get_submit_proposal_msg("01234-ABCDE");
        process_admin_event(message, &pool, "", "", "", "", reactor.igniter())
            .expect("Error processing message");

        let gamerooms = query_gameroom_table(&pool);

        assert_eq!(gamerooms.len(), 1);

        let gameroom = &gamerooms[0];
        let expected_gameroom = get_gameroom("01234-ABCDE", SystemTime::now());

        assert_eq!(gameroom.circuit_id, expected_gameroom.circuit_id);
        assert_eq!(
            gameroom.authorization_type,
            expected_gameroom.authorization_type
        );
        assert_eq!(gameroom.persistence, expected_gameroom.persistence);
        assert_eq!(gameroom.routes, expected_gameroom.routes);
        assert_eq!(gameroom.durability, expected_gameroom.durability);
        assert_eq!(
            gameroom.circuit_management_type,
            expected_gameroom.circuit_management_type
        );
        assert_eq!(gameroom.alias, expected_gameroom.alias);
        assert_eq!(gameroom.status, expected_gameroom.status);
    }

    #[test]
    /// Tests if when receiving an admin message to CreateProposal the gameroom_member
    /// table is updated as expected
    fn test_process_proposal_submitted_message_update_member_table() {
        let reactor = Reactor::new();
        let pool: ConnectionPool = gameroom_database::create_connection_pool(DATABASE_URL)
            .expect("Failed to get database connection pool");

        clear_gameroom_table(&pool);
        clear_gameroom_notification_table(&pool);

        let message = get_submit_proposal_msg("01234-ABCDE");
        process_admin_event(message, &pool, "", "", "", "", reactor.igniter())
            .expect("Error processing message");

        let members = query_gameroom_members_table(&pool);

        assert_eq!(members.len(), 1);

        let node = &members[0];
        let expected_node = get_new_gameroom_member("01234-ABCDE", SystemTime::now());

        assert_eq!(node.node_id, expected_node.node_id);
        assert_eq!(node.endpoints, expected_node.endpoints);
    }

    #[test]
    /// Tests if when receiving an admin message to CreateProposal the gameroom_service
    /// table is updated as expected
    fn test_process_proposal_submitted_message_update_service_table() {
        let reactor = Reactor::new();
        let pool: ConnectionPool = gameroom_database::create_connection_pool(DATABASE_URL)
            .expect("Failed to get database connection pool");

        clear_gameroom_table(&pool);
        clear_gameroom_notification_table(&pool);

        let message = get_submit_proposal_msg("01234-ABCDE");
        process_admin_event(message, &pool, "", "", "", "", reactor.igniter())
            .expect("Error processing message");

        let services = query_gameroom_service_table(&pool);

        assert_eq!(services.len(), 1);

        let service = &services[0];
        let expected_service = get_new_gameroom_service("01234-ABCDE", SystemTime::now());

        assert_eq!(service.service_id, expected_service.service_id);
        assert_eq!(service.service_type, expected_service.service_type);
        assert_eq!(service.allowed_nodes, expected_service.allowed_nodes);
    }

    #[test]
    /// Tests if when receiving an admin message to CreateProposal the gameroom_notification
    /// table is updated as expected
    fn test_process_proposal_submitted_message_update_notification_table() {
        let reactor = Reactor::new();
        let pool: ConnectionPool = gameroom_database::create_connection_pool(DATABASE_URL)
            .expect("Failed to get database connection pool");

        clear_gameroom_table(&pool);
        clear_gameroom_notification_table(&pool);

        let message = get_submit_proposal_msg("01234-ABCDE");
        process_admin_event(message, &pool, "", "", "", "", reactor.igniter())
            .expect("Error processing message");

        let notifications = query_gameroom_notification_table(&pool);

        assert_eq!(notifications.len(), 1);

        let notification = &notifications[0];
        let expected_notification =
            get_new_gameroom_notification_proposal("01234-ABCDE", SystemTime::now());

        assert_eq!(
            notification.notification_type,
            expected_notification.notification_type
        );
        assert_eq!(notification.requester, expected_notification.requester);
        assert_eq!(notification.target, expected_notification.target);
        assert_eq!(notification.read, expected_notification.read);
    }

    #[test]
    /// Tests if when receiving an admin message ProposalAccepted the gameroom_proposal
    /// table is updated as expected
    fn test_process_proposal_accepted_message_ok() {
        let reactor = Reactor::new();
        let pool: ConnectionPool = gameroom_database::create_connection_pool(DATABASE_URL)
            .expect("Failed to get database connection pool");

        clear_gameroom_table(&pool);
        clear_gameroom_notification_table(&pool);

        let created_time = SystemTime::now();

        // insert gameroom into database
        insert_gameroom_table(&pool, get_gameroom("01234-ABCDE", created_time.clone()));

        // insert pending proposal into database
        insert_proposals_table(
            &pool,
            get_gameroom_proposal("01234-ABCDE", created_time.clone()),
        );

        insert_member_table(
            &pool,
            get_new_gameroom_member("01234-ABCDE", created_time.clone()),
        );
        insert_service_table(
            &pool,
            get_new_gameroom_service("01234-ABCDE", created_time.clone()),
        );

        let accept_message = get_accept_proposal_msg("01234-ABCDE");

        // accept proposal
        process_admin_event(accept_message, &pool, "", "", "", "", reactor.igniter())
            .expect("Error processing message");

        let proposals = query_proposals_table(&pool);

        assert_eq!(proposals.len(), 1);

        let proposal = &proposals[0];

        // Check proposal updated_time changed
        assert!(proposal.updated_time > created_time);
        // Check status was changed to accepted
        assert_eq!(proposal.status, "Accepted");

        let members = query_gameroom_members_table(&pool);

        assert_eq!(members.len(), 1);

        let member = &members[0];

        // Check member updated_time changed
        assert!(member.updated_time > created_time);
        // Check status was changed to accepted
        assert_eq!(member.status, "Accepted");

        let services = query_gameroom_service_table(&pool);

        assert_eq!(services.len(), 1);

        let service = &services[0];

        // Check service updated_time changed
        assert!(service.updated_time > created_time);
        // Check status was changed to accepted
        assert_eq!(service.status, "Accepted");
    }

    #[test]
    /// Tests if when receiving an admin message ProposalAccepted an error is returned
    /// if a pending proposal for that circuit is not found
    fn test_process_proposal_accepted_message_err() {
        let reactor = Reactor::new();
        let pool: ConnectionPool = gameroom_database::create_connection_pool(DATABASE_URL)
            .expect("Failed to get database connection pool");

        clear_gameroom_table(&pool);
        clear_gameroom_notification_table(&pool);

        let accept_message = get_accept_proposal_msg("01234-ABCDE");

        // accept proposal
        match process_admin_event(accept_message, &pool, "", "", "", "", reactor.igniter()) {
            Ok(()) => panic!("Pending proposal for circuit is missing, error should be returned"),
            Err(AppAuthHandlerError::Database(msg)) => {
                assert!(msg.contains("Could not find open proposal for circuit: 01234-ABCDE"));
            }
            Err(err) => panic!("Should have gotten Database error but got {}", err),
        }
    }

    #[test]
    /// Tests if when receiving an admin message ProposalRejected the gameroom_proposal and
    /// gameroom tables are updated as expected
    fn test_process_proposal_rejected_message_ok() {
        let reactor = Reactor::new();
        let pool: ConnectionPool = gameroom_database::create_connection_pool(DATABASE_URL)
            .expect("Failed to get database connection pool");

        clear_gameroom_table(&pool);
        clear_gameroom_notification_table(&pool);

        let created_time = SystemTime::now();

        // insert gameroom into database
        insert_gameroom_table(&pool, get_gameroom("01234-ABCDE", created_time.clone()));

        // insert pending proposal into database
        insert_proposals_table(
            &pool,
            get_gameroom_proposal("01234-ABCDE", created_time.clone()),
        );

        insert_member_table(
            &pool,
            get_new_gameroom_member("01234-ABCDE", created_time.clone()),
        );
        insert_service_table(
            &pool,
            get_new_gameroom_service("01234-ABCDE", created_time.clone()),
        );

        let rejected_message = get_reject_proposal_msg("01234-ABCDE");

        // reject proposal
        process_admin_event(rejected_message, &pool, "", "", "", "", reactor.igniter())
            .expect("Error processing message");

        let proposals = query_proposals_table(&pool);

        assert_eq!(proposals.len(), 1);

        let proposal = &proposals[0];

        // Check proposal updated_time changed
        assert!(proposal.updated_time > created_time);
        // Check status was changed to rejected
        assert_eq!(proposal.status, "Rejected");

        let gamerooms = query_gameroom_table(&pool);

        assert_eq!(gamerooms.len(), 1);

        let gameroom = &gamerooms[0];

        // Check gameroom updated_time changed
        assert!(gameroom.updated_time > created_time);
        // Check status was changed to rejected
        assert_eq!(gameroom.status, "Rejected");

        let members = query_gameroom_members_table(&pool);

        assert_eq!(members.len(), 1);

        let member = &members[0];

        // Check member updated_time changed
        assert!(member.updated_time > created_time);
        // Check status was changed to rejected
        assert_eq!(member.status, "Rejected");

        let services = query_gameroom_service_table(&pool);

        assert_eq!(services.len(), 1);

        let service = &services[0];

        // Check service updated_time changed
        assert!(service.updated_time > created_time);
        // Check status was changed to rejected
        assert_eq!(service.status, "Rejected");
    }

    #[test]
    /// Tests if when receiving an admin message ProposalRejected an error is returned
    /// if a pending proposal for that circuit is not found
    fn test_process_proposal_rejected_message_err() {
        let reactor = Reactor::new();
        let pool: ConnectionPool = gameroom_database::create_connection_pool(DATABASE_URL)
            .expect("Failed to get database connection pool");

        clear_gameroom_table(&pool);
        clear_gameroom_notification_table(&pool);

        let rejected_message = get_reject_proposal_msg("01234-ABCDE");

        // reject proposal
        match process_admin_event(rejected_message, &pool, "", "", "", "", reactor.igniter()) {
            Ok(()) => panic!("Pending proposal for circuit is missing, error should be returned"),
            Err(AppAuthHandlerError::Database(msg)) => {
                assert!(msg.contains("Could not find open proposal for circuit: 01234-ABCDE"));
            }
            Err(err) => panic!("Should have gotten Database error but got {}", err),
        }
    }

    #[test]
    /// Tests if when receiving an admin message ProposalVote the gameroom_proposal and
    /// proposal_vote_record tables are updated as expected
    fn test_process_proposal_vote_message_ok() {
        let reactor = Reactor::new();
        let pool: ConnectionPool = gameroom_database::create_connection_pool(DATABASE_URL)
            .expect("Failed to get database connection pool");

        clear_gameroom_table(&pool);
        clear_gameroom_notification_table(&pool);

        let created_time = SystemTime::now();

        // insert gameroom into database
        insert_gameroom_table(&pool, get_gameroom("01234-ABCDE", created_time.clone()));

        // insert pending proposal into database
        insert_proposals_table(
            &pool,
            get_gameroom_proposal("01234-ABCDE", created_time.clone()),
        );

        let vote_message = get_vote_proposal_msg("01234-ABCDE");

        // vote proposal
        process_admin_event(vote_message, &pool, "", "", "", "", reactor.igniter())
            .expect("Error processing message");

        let proposals = query_proposals_table(&pool);

        assert_eq!(proposals.len(), 1);

        let proposal = &proposals[0];

        // Check proposal updated_time changed
        assert!(proposal.updated_time > created_time);

        let votes = query_votes_table(&pool);
        assert_eq!(votes.len(), 1);

        let vote = &votes[0];
        let expected_vote = get_new_vote_record(proposal.id, SystemTime::now());
        assert_eq!(vote.voter_public_key, expected_vote.voter_public_key);
        assert_eq!(vote.vote, expected_vote.vote);
        assert_eq!(vote.created_time, proposal.updated_time);
    }

    #[test]
    /// Tests if when receiving an admin message to ProposalVote the gameroom_notification
    /// table is updated as expected
    fn test_process_proposal_vote_message_update_notification_table() {
        let reactor = Reactor::new();
        let pool: ConnectionPool = gameroom_database::create_connection_pool(DATABASE_URL)
            .expect("Failed to get database connection pool");

        clear_gameroom_table(&pool);
        clear_gameroom_notification_table(&pool);

        let created_time = SystemTime::now();

        // insert gameroom into database
        insert_gameroom_table(&pool, get_gameroom("01234-ABCDE", created_time.clone()));

        // insert pending proposal into database
        insert_proposals_table(
            &pool,
            get_gameroom_proposal("01234-ABCDE", created_time.clone()),
        );

        let vote_message = get_vote_proposal_msg("01234-ABCDE");

        // vote proposal
        process_admin_event(vote_message, &pool, "", "", "", "", reactor.igniter())
            .expect("Error processing message");

        let notifications = query_gameroom_notification_table(&pool);

        assert_eq!(notifications.len(), 1);

        let votes = query_votes_table(&pool);
        assert_eq!(votes.len(), 1);

        let _vote = &votes[0];

        let notification = &notifications[0];
        let expected_notification =
            get_new_gameroom_notification_vote("01234-ABCDE", SystemTime::now());

        assert_eq!(
            notification.notification_type,
            expected_notification.notification_type
        );
        assert_eq!(notification.requester, expected_notification.requester);
        assert_eq!(notification.target, expected_notification.target);
        assert_eq!(notification.read, expected_notification.read);
    }

    #[test]
    /// Tests if when receiving an admin message ProposalVote an error is returned
    /// if a pending proposal for that circuit is not found
    fn test_process_proposal_vote_message_err() {
        let reactor = Reactor::new();
        let pool: ConnectionPool = gameroom_database::create_connection_pool(DATABASE_URL)
            .expect("Failed to get database connection pool");

        clear_gameroom_table(&pool);
        clear_gameroom_notification_table(&pool);

        let vote_message = get_vote_proposal_msg("01234-ABCDE");

        // vote proposal
        match process_admin_event(vote_message, &pool, "", "", "", "", reactor.igniter()) {
            Ok(()) => panic!("Pending proposal for circuit is missing, error should be returned"),
            Err(AppAuthHandlerError::Database(msg)) => {
                assert!(msg.contains("Could not find open proposal for circuit: 01234-ABCDE"));
            }
            Err(err) => panic!("Should have gotten Database error but got {}", err),
        }
    }

    #[test]
    /// Tests if the admin message CreateProposal to a database GameroomProposal is successful
    fn test_parse_proposal() {
        let time = SystemTime::now();
        let proposal = parse_proposal(
            &get_msg_proposal("01234-ABCDE"),
            time.clone(),
            to_hex(&public_key()),
        );

        assert_eq!(proposal, get_gameroom_proposal("01234-ABCDE", time.clone()));
    }

    #[test]
    /// Tests if the admin message CreateCircuit to a database Gameroom is successful
    fn test_parse_gameroom() {
        let time = SystemTime::now();
        let gameroom = parse_gameroom(&get_create_circuit_msg("01234-ABCDE"), time)
            .expect("Failed to parse gameroom");

        assert_eq!(gameroom, get_gameroom("01234-ABCDE", time.clone()))
    }

    #[test]
    /// Tests if the admin message SplinterService to a database NewGameroomService is successful
    fn test_parse_gameroom_service() {
        let time = SystemTime::now();
        let services = parse_splinter_services(
            "01234-ABCDE",
            &get_msg_proposal("01234-ABCDE").circuit.roster,
            time,
        );

        assert_eq!(
            services,
            vec![get_new_gameroom_service("01234-ABCDE", time)]
        );
    }

    #[test]
    /// Tests if the admin message SplinterNode to a database NewGameroomMember is successful
    fn test_parse_gameroom_member() {
        let time = SystemTime::now();
        let members = parse_splinter_nodes(
            "01234-ABCDE",
            &get_msg_proposal("01234-ABCDE").circuit.members,
            time,
        );

        assert_eq!(members, vec![get_new_gameroom_member("01234-ABCDE", time)]);
    }

    fn get_create_circuit_msg(circuit_id: &str) -> CreateCircuit {
        let mut arguments = vec![];
        arguments.push(("test_key".to_string(), "test_value".to_string()));
        let application_metadata = ApplicationMetadata::new("test_gameroom", vec![].as_slice())
            .to_bytes()
            .expect("Failed to serialize application_metadata");
        CreateCircuit {
            circuit_id: circuit_id.to_string(),
            roster: vec![SplinterService {
                service_id: "gr00".to_string(),
                service_type: "scabbard".to_string(),
                allowed_nodes: vec!["acme_corp".to_string()],
                arguments: arguments,
            }],
            members: vec![SplinterNode {
                node_id: "Node-123".to_string(),
                endpoints: vec!["127.0.0.1:8282".to_string()],
            }],
            authorization_type: AuthorizationType::Trust,
            persistence: PersistenceType::Any,
            durability: DurabilityType::NoDurability,
            routes: RouteType::Any,
            circuit_management_type: "gameroom".to_string(),
            application_metadata,
            comments: "test circuit".to_string(),
        }
    }

    fn get_msg_proposal(circuit_id: &str) -> CircuitProposal {
        CircuitProposal {
            proposal_type: ProposalType::Create,
            circuit_id: circuit_id.to_string(),
            circuit_hash: "8e066d41911817a42ab098eda35a2a2b11e93c753bc5ecc3ffb3e99ed99ada0d"
                .to_string(),
            circuit: get_create_circuit_msg(circuit_id),
            votes: vec![],
            requester: public_key(),
            requester_node_id: "acme_corp".to_string(),
        }
    }

    fn get_msg_proposal_with_vote(circuit_id: &str) -> CircuitProposal {
        let vote = VoteRecord {
            public_key: public_key(),
            vote: Vote::Accept,
            voter_node_id: "acme_corp".to_string(),
        };

        CircuitProposal {
            proposal_type: ProposalType::Create,
            circuit_id: circuit_id.to_string(),
            circuit_hash: "8e066d41911817a42ab098eda35a2a2b11e93c753bc5ecc3ffb3e99ed99ada0d"
                .to_string(),
            circuit: get_create_circuit_msg(circuit_id),
            votes: vec![vote],
            requester: public_key(),
            requester_node_id: "acme_corp".to_string(),
        }
    }

    fn get_reject_proposal_msg(circuit_id: &str) -> Event {
        Event {
            timestamp: current_time_millis(),
            admin_event: AdminServiceEvent::ProposalRejected((
                get_msg_proposal_with_vote(circuit_id),
                public_key(),
            )),
        }
    }

    fn get_accept_proposal_msg(circuit_id: &str) -> Event {
        Event {
            timestamp: current_time_millis(),
            admin_event: AdminServiceEvent::ProposalAccepted((
                get_msg_proposal_with_vote(circuit_id),
                public_key(),
            )),
        }
    }

    fn get_vote_proposal_msg(circuit_id: &str) -> Event {
        Event {
            timestamp: current_time_millis(),
            admin_event: AdminServiceEvent::ProposalVote((
                get_msg_proposal_with_vote(circuit_id),
                public_key(),
            )),
        }
    }

    fn get_submit_proposal_msg(circuit_id: &str) -> Event {
        Event {
            timestamp: current_time_millis(),
            admin_event: AdminServiceEvent::ProposalSubmitted(get_msg_proposal(circuit_id)),
        }
    }

    fn get_gameroom_proposal(circuit_id: &str, timestamp: SystemTime) -> NewGameroomProposal {
        NewGameroomProposal {
            proposal_type: "Create".to_string(),
            circuit_id: circuit_id.to_string(),
            circuit_hash: "8e066d41911817a42ab098eda35a2a2b11e93c753bc5ecc3ffb3e99ed99ada0d"
                .to_string(),
            requester: to_hex(&public_key()),
            requester_node_id: "acme_corp".to_string(),
            status: "Pending".to_string(),
            created_time: timestamp,
            updated_time: timestamp,
        }
    }

    fn get_gameroom(circuit_id: &str, timestamp: SystemTime) -> Gameroom {
        Gameroom {
            circuit_id: circuit_id.to_string(),
            authorization_type: "Trust".to_string(),
            persistence: "Any".to_string(),
            durability: "NoDurability".to_string(),
            routes: "Any".to_string(),
            circuit_management_type: "gameroom".to_string(),
            alias: "test_gameroom".to_string(),
            status: "Pending".to_string(),
            created_time: timestamp,
            updated_time: timestamp,
        }
    }

    fn get_new_vote_record(proposal_id: i64, timestamp: SystemTime) -> NewProposalVoteRecord {
        NewProposalVoteRecord {
            proposal_id,
            voter_public_key: to_hex(&public_key()),
            voter_node_id: "acme_corp".to_string(),
            vote: "Accept".to_string(),
            created_time: timestamp,
        }
    }

    fn get_new_gameroom_service(circuit_id: &str, timestamp: SystemTime) -> NewGameroomService {
        NewGameroomService {
            circuit_id: circuit_id.to_string(),
            service_id: "gr00".to_string(),
            service_type: "scabbard".to_string(),
            allowed_nodes: vec!["acme_corp".to_string()],
            arguments: vec![json!({
                "key": "test_key",
                "value": "test_value"
            })],
            status: "Pending".to_string(),
            last_event: "".to_string(),
            created_time: timestamp,
            updated_time: timestamp,
        }
    }

    fn get_new_gameroom_member(circuit_id: &str, timestamp: SystemTime) -> NewGameroomMember {
        NewGameroomMember {
            circuit_id: circuit_id.to_string(),
            node_id: "Node-123".to_string(),
            endpoints: vec!["127.0.0.1:8282".to_string()],
            status: "Pending".to_string(),
            created_time: timestamp,
            updated_time: timestamp,
        }
    }

    fn get_new_gameroom_notification_proposal(
        circuit_id: &str,
        timestamp: SystemTime,
    ) -> NewGameroomNotification {
        NewGameroomNotification {
            notification_type: "gameroom_proposal".to_string(),
            requester: to_hex(&public_key()),
            requester_node_id: "acme_corp".to_string(),
            target: circuit_id.to_string(),
            created_time: timestamp,
            read: false,
        }
    }

    fn get_new_gameroom_notification_vote(
        circuit_id: &str,
        timestamp: SystemTime,
    ) -> NewGameroomNotification {
        NewGameroomNotification {
            notification_type: "proposal_vote_record".to_string(),
            requester: to_hex(&public_key()),
            requester_node_id: "acme_corp".to_string(),
            target: circuit_id.to_string(),
            created_time: timestamp,
            read: false,
        }
    }

    fn query_votes_table(pool: &ConnectionPool) -> Vec<ProposalVoteRecord> {
        use gameroom_database::schema::proposal_vote_record;

        let conn = &*pool.get().expect("Error getting db connection");
        proposal_vote_record::table
            .select(proposal_vote_record::all_columns)
            .load::<ProposalVoteRecord>(conn)
            .expect("Error fetching vote records")
    }

    fn query_gameroom_members_table(pool: &ConnectionPool) -> Vec<GameroomMember> {
        use gameroom_database::schema::gameroom_member;

        let conn = &*pool.get().expect("Error getting db connection");
        gameroom_member::table
            .select(gameroom_member::all_columns)
            .load::<GameroomMember>(conn)
            .expect("Error fetching circuit members")
    }

    fn query_gameroom_service_table(pool: &ConnectionPool) -> Vec<GameroomService> {
        use gameroom_database::schema::gameroom_service;

        let conn = &*pool.get().expect("Error getting db connection");
        gameroom_service::table
            .select(gameroom_service::all_columns)
            .load::<GameroomService>(conn)
            .expect("Error fetching circuit members")
    }

    fn query_proposals_table(pool: &ConnectionPool) -> Vec<GameroomProposal> {
        use gameroom_database::schema::gameroom_proposal;

        let conn = &*pool.get().expect("Error getting db connection");
        gameroom_proposal::table
            .select(gameroom_proposal::all_columns)
            .load::<GameroomProposal>(conn)
            .expect("Error fetching proposals")
    }

    fn query_gameroom_table(pool: &ConnectionPool) -> Vec<Gameroom> {
        use gameroom_database::schema::gameroom;

        let conn = &*pool.get().expect("Error getting db connection");
        gameroom::table
            .select(gameroom::all_columns)
            .load::<Gameroom>(conn)
            .expect("Error fetching proposals")
    }

    fn query_gameroom_notification_table(pool: &ConnectionPool) -> Vec<GameroomNotification> {
        use gameroom_database::schema::gameroom_notification;

        let conn = &*pool.get().expect("Error getting db connection");
        gameroom_notification::table
            .select(gameroom_notification::all_columns)
            .load::<GameroomNotification>(conn)
            .expect("Error fetching proposals")
    }

    fn insert_proposals_table(pool: &ConnectionPool, proposal: NewGameroomProposal) {
        use gameroom_database::schema::gameroom_proposal;

        let conn = &*pool.get().expect("Error getting db connection");
        insert_into(gameroom_proposal::table)
            .values(&vec![proposal])
            .execute(conn)
            .map(|_| ())
            .expect("Failed to insert proposal in table")
    }

    fn insert_gameroom_table(pool: &ConnectionPool, gameroom: Gameroom) {
        use gameroom_database::schema::gameroom;

        let conn = &*pool.get().expect("Error getting db connection");
        insert_into(gameroom::table)
            .values(&vec![gameroom])
            .execute(conn)
            .map(|_| ())
            .expect("Failed to insert proposal in table")
    }

    fn insert_member_table(pool: &ConnectionPool, member: NewGameroomMember) {
        use gameroom_database::schema::gameroom_member;

        let conn = &*pool.get().expect("Error getting db connection");
        insert_into(gameroom_member::table)
            .values(&vec![member])
            .execute(conn)
            .map(|_| ())
            .expect("Failed to insert proposal in table")
    }

    fn insert_service_table(pool: &ConnectionPool, service: NewGameroomService) {
        use gameroom_database::schema::gameroom_service;

        let conn = &*pool.get().expect("Error getting db connection");
        insert_into(gameroom_service::table)
            .values(&vec![service])
            .execute(conn)
            .map(|_| ())
            .expect("Failed to insert proposal in table")
    }

    fn clear_gameroom_table(pool: &ConnectionPool) {
        use gameroom_database::schema::gameroom::dsl::*;

        let conn = &*pool.get().expect("Error getting db connection");
        diesel::delete(gameroom)
            .execute(conn)
            .expect("Error cleaning gameroom table");
    }

    fn clear_gameroom_notification_table(pool: &ConnectionPool) {
        use gameroom_database::schema::gameroom_notification::dsl::*;

        let conn = &*pool.get().expect("Error getting db connection");
        diesel::delete(gameroom_notification)
            .execute(conn)
            .expect("Error cleaning gameroom_notification table");
    }

    fn public_key() -> Vec<u8> {
        vec![73, 119, 65, 65, 65, 81]
    }

    fn current_time_millis() -> u64 {
        let now = SystemTime::now();
        let duration = now.duration_since(SystemTime::UNIX_EPOCH).unwrap();
        duration.as_millis() as u64
    }
}

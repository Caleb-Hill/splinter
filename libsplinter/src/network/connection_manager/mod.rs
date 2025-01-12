// Copyright 2018-2022 Cargill Incorporated
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

pub mod authorizers;
mod builder;
mod error;
mod notification;

use std::cmp::min;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::thread;
use std::time::Instant;

use uuid::Uuid;

pub use builder::ConnectionManagerBuilder;
pub use error::{AuthorizerError, ConnectionManagerError};
pub use notification::ConnectionManagerNotification;

use crate::error::InternalError;
use crate::network::auth::ConnectionAuthorizationType;
use crate::threading::lifecycle::ShutdownHandle;
use crate::threading::pacemaker;
use crate::transport::matrix::{ConnectionMatrixLifeCycle, ConnectionMatrixSender};
use crate::transport::{ConnectError, Connection, Transport};

const INITIAL_RETRY_FREQUENCY: u64 = 10;

pub type AuthorizerCallback =
    Box<dyn Fn(AuthorizationResult) -> Result<(), Box<dyn std::error::Error>> + Send>;

pub trait Authorizer {
    fn authorize_connection(
        &self,
        connection_id: String,
        connection: Box<dyn Connection>,
        on_complete: AuthorizerCallback,
        expected_authorization: Option<ConnectionAuthorizationType>,
        local_authorization: Option<ConnectionAuthorizationType>,
    ) -> Result<(), AuthorizerError>;
}

pub enum AuthorizationResult {
    Authorized {
        connection_id: String,
        identity: ConnectionAuthorizationType,
        connection: Box<dyn Connection>,
        expected_authorization: ConnectionAuthorizationType,
        local_authorization: ConnectionAuthorizationType,
    },
    Unauthorized {
        connection_id: String,
        connection: Box<dyn Connection>,
    },
}

pub type SubscriberId = usize;
type Subscriber =
    Box<dyn Fn(ConnectionManagerNotification) -> Result<(), Box<dyn std::error::Error>> + Send>;

/// Responsible for broadcasting connection manager notifications.
struct SubscriberMap {
    subscribers: HashMap<SubscriberId, Subscriber>,
    next_id: SubscriberId,
}

impl SubscriberMap {
    fn new() -> Self {
        Self {
            subscribers: HashMap::new(),
            next_id: 0,
        }
    }

    fn broadcast(&mut self, notification: ConnectionManagerNotification) {
        let mut failures = vec![];
        for (id, callback) in self.subscribers.iter() {
            if let Err(err) = (*callback)(notification.clone()) {
                failures.push(*id);
                debug!("Dropping subscriber ({}): {}", id, err);
            }
        }

        for id in failures {
            self.subscribers.remove(&id);
        }
    }

    fn add_subscriber(&mut self, subscriber: Subscriber) -> SubscriberId {
        let subscriber_id = self.next_id;
        self.next_id += 1;
        self.subscribers.insert(subscriber_id, subscriber);

        subscriber_id
    }

    fn remove_subscriber(&mut self, subscriber_id: SubscriberId) {
        self.subscribers.remove(&subscriber_id);
    }
}

/// Messages handled by the connection manager.
enum CmMessage {
    Shutdown,
    Request(CmRequest),
    AuthResult(AuthResult),
    SendHeartbeats,
}

/// CmMessages sent by a Connector.
enum CmRequest {
    RequestOutboundConnection {
        endpoint: String,
        connection_id: String,
        expected_authorization: Option<ConnectionAuthorizationType>,
        local_authorization: Option<ConnectionAuthorizationType>,
        sender: Sender<Result<(), ConnectionManagerError>>,
    },
    RemoveConnection {
        endpoint: String,
        connection_id: String,
        sender: Sender<Result<Option<String>, ConnectionManagerError>>,
    },
    ListConnections {
        sender: Sender<Result<Vec<String>, ConnectionManagerError>>,
    },
    AddInboundConnection {
        connection: Box<dyn Connection>,
        sender: Sender<Result<(), ConnectionManagerError>>,
    },
    Subscribe {
        sender: Sender<Result<SubscriberId, ConnectionManagerError>>,
        callback: Subscriber,
    },
    Unsubscribe {
        subscriber_id: SubscriberId,
        sender: Sender<Result<(), ConnectionManagerError>>,
    },
}

/// Messages sent to ConnectionState to report on the status of a connection
/// authorization attempt.
enum AuthResult {
    Outbound {
        endpoint: String,
        auth_result: AuthorizationResult,
    },
    Inbound {
        endpoint: String,
        auth_result: AuthorizationResult,
    },
}

/// Creates, manages, and maintains connections. A connection manager
/// guarantees that the connections it creates will be maintained via
/// reconnections. This is not true for external connections.
pub struct ConnectionManager {
    pacemaker: pacemaker::Pacemaker,
    join_handle: thread::JoinHandle<()>,
    sender: Sender<CmMessage>,
}

impl ConnectionManager {
    /// Construct a new `ConnectionManagerBuilder` for creating a new `ConnectionManager` instance.
    pub fn builder<T, U>() -> ConnectionManagerBuilder<T, U>
    where
        T: ConnectionMatrixLifeCycle + 'static,
        U: ConnectionMatrixSender + 'static,
    {
        ConnectionManagerBuilder::new()
    }

    /// Create a new connector for performing client operations on this instance's state.
    pub fn connector(&self) -> Connector {
        Connector {
            sender: self.sender.clone(),
        }
    }
}

/// Connector is a client or handle to the connection manager and is used to
/// send request to the connection manager.
#[derive(Clone)]
pub struct Connector {
    sender: Sender<CmMessage>,
}

impl Connector {
    /// Request a connection to the given endpoint with a provided connection ID.
    ///
    /// This operation is idempotent: if a connection to that connection ID already exists, a new
    /// connection is not created. On successful connection Ok is returned. The connection is not
    /// ready to use, it must complete authorization. When the connection is ready a
    /// `ConnectionManagerNotification::Connected`will be sent to subscribers.
    ///
    /// # Errors
    ///
    /// An error is returned if the connection cannot be created.
    pub fn request_connection(
        &self,
        endpoint: &str,
        connection_id: &str,
        expected_authorization: Option<ConnectionAuthorizationType>,
        local_authorization: Option<ConnectionAuthorizationType>,
    ) -> Result<(), ConnectionManagerError> {
        let (sender, recv) = channel();
        self.sender
            .send(CmMessage::Request(CmRequest::RequestOutboundConnection {
                sender,
                endpoint: endpoint.to_string(),
                connection_id: connection_id.into(),
                expected_authorization,
                local_authorization,
            }))
            .map_err(|_| {
                ConnectionManagerError::SendMessageError(
                    "The connection manager is no longer running: unable to send request".into(),
                )
            })?;

        recv.recv().map_err(|_| {
            ConnectionManagerError::SendMessageError(
                "The connection manager is no longer running: could not receive response".into(),
            )
        })?
    }

    /// Removes a connection from a connection manager.
    ///
    /// # Returns
    ///
    /// The endpoint, if the connection exists; None, otherwise.
    ///
    /// # Errors
    ///
    /// Returns a ConnectionManagerError if the query cannot be performed.
    pub fn remove_connection(
        &self,
        endpoint: &str,
        connection_id: &str,
    ) -> Result<Option<String>, ConnectionManagerError> {
        let (sender, recv) = channel();
        self.sender
            .send(CmMessage::Request(CmRequest::RemoveConnection {
                sender,
                endpoint: endpoint.to_string(),
                connection_id: connection_id.to_string(),
            }))
            .map_err(|_| {
                ConnectionManagerError::SendMessageError(
                    "The connection manager is no longer running".into(),
                )
            })?;

        recv.recv().map_err(|_| {
            ConnectionManagerError::SendMessageError(
                "The connection manager is no longer running".into(),
            )
        })?
    }

    /// Subscribe to notifications for connection events.
    ///
    /// ConnectionManagerNotification instances will be transformed via type `T`'s implementation
    /// of `From<ConnectionManagerNotification>` and passed to the given sender.
    ///
    /// # Returns
    ///
    /// The subscriber id that can be used for unsubscribing the given sender.
    ///
    /// # Errors
    ///
    /// Return a ConnectionManagerError if the subscriber cannot be registered via the Connector
    /// instance.
    pub fn subscribe<T>(
        &self,
        subscriber: Sender<T>,
    ) -> Result<SubscriberId, ConnectionManagerError>
    where
        T: From<ConnectionManagerNotification> + Send + 'static,
    {
        let (sender, recv) = channel();
        self.sender
            .send(CmMessage::Request(CmRequest::Subscribe {
                sender,
                callback: Box::new(move |notification| {
                    subscriber.send(T::from(notification)).map_err(Box::from)
                }),
            }))
            .map_err(|_| {
                ConnectionManagerError::SendMessageError(
                    "The connection manager is no longer running".into(),
                )
            })?;

        recv.recv().map_err(|_| {
            ConnectionManagerError::SendMessageError(
                "The connection manager is no longer running".into(),
            )
        })?
    }

    /// Unsubscribe to connection manager notifications.
    ///
    /// # Errors
    ///
    /// Returns a ConnectionManagerError if the connection manager
    /// has stopped running.
    pub fn unsubscribe(&self, subscriber_id: SubscriberId) -> Result<(), ConnectionManagerError> {
        let (sender, recv) = channel();
        self.sender
            .send(CmMessage::Request(CmRequest::Unsubscribe {
                subscriber_id,
                sender,
            }))
            .map_err(|_| {
                ConnectionManagerError::SendMessageError(
                    "The connection manager is no longer running".into(),
                )
            })?;

        recv.recv().map_err(|_| {
            ConnectionManagerError::SendMessageError(
                "The connection manager is no longer running".into(),
            )
        })?
    }

    /// List the connections available to this Connector instance.
    ///
    /// # Returns
    ///
    /// Returns a vector of connection endpoints.
    ///
    /// # Errors
    ///
    /// Returns a ConnectionManagerError if the connections cannot be queried.
    pub fn list_connections(&self) -> Result<Vec<String>, ConnectionManagerError> {
        let (sender, recv) = channel();
        self.sender
            .send(CmMessage::Request(CmRequest::ListConnections { sender }))
            .map_err(|_| {
                ConnectionManagerError::SendMessageError(
                    "The connection manager is no longer running".into(),
                )
            })?;

        recv.recv().map_err(|_| {
            ConnectionManagerError::SendMessageError(
                "The connection manager is no longer running".into(),
            )
        })?
    }

    /// Add a new inbound connection.
    ///
    /// # Error
    ///
    /// Returns a ConnectionManagerError if the connection manager is
    /// no longer running.
    pub fn add_inbound_connection(
        &self,
        connection: Box<dyn Connection>,
    ) -> Result<(), ConnectionManagerError> {
        let (sender, recv) = channel();
        self.sender
            .send(CmMessage::Request(CmRequest::AddInboundConnection {
                connection,
                sender,
            }))
            .map_err(|_| {
                ConnectionManagerError::SendMessageError(
                    "The connection manager is no longer running".into(),
                )
            })?;

        recv.recv().map_err(|_| {
            ConnectionManagerError::SendMessageError(
                "The connection manager is no longer running".into(),
            )
        })?
    }
}

impl ShutdownHandle for ConnectionManager {
    fn signal_shutdown(&mut self) {
        self.pacemaker.shutdown_signaler().shutdown();

        if self.sender.send(CmMessage::Shutdown).is_err() {
            warn!("Connection manager is no longer running");
        }
    }

    fn wait_for_shutdown(self) -> Result<(), InternalError> {
        debug!("Shutting down connection manager pacemaker...");
        self.pacemaker.await_shutdown();
        debug!("Shutting down connection manager pacemaker (complete)");

        self.join_handle.join().map_err(|err| {
            InternalError::with_message(format!(
                "Connection manager thread did not shutdown correctly: {:?}",
                err
            ))
        })
    }
}

/// Metadata describing a connection managed by the connection manager.
#[derive(Clone, Debug)]
struct ConnectionMetadata {
    connection_id: String,
    endpoint: String,
    identity: ConnectionAuthorizationType,
    extended_metadata: ConnectionMetadataExt,
}

impl ConnectionMetadata {
    fn is_outbound(&self) -> bool {
        matches!(
            self.extended_metadata,
            ConnectionMetadataExt::Outbound { .. }
        )
    }

    fn connection_id(&self) -> &str {
        &self.connection_id
    }

    fn endpoint(&self) -> &str {
        &self.endpoint
    }

    fn identity(&self) -> &ConnectionAuthorizationType {
        &self.identity
    }
}

/// Enum describing metadata that is specific to the two different connection
/// types, outbound and inbound.
#[derive(Clone, Debug)]
enum ConnectionMetadataExt {
    Outbound {
        reconnecting: bool,
        retry_frequency: u64,
        last_connection_attempt: Instant,
        reconnection_attempts: u64,
        expected_authorization: ConnectionAuthorizationType,
        local_authorization: ConnectionAuthorizationType,
    },
    Inbound {
        disconnected: bool,
        local_authorization: ConnectionAuthorizationType,
    },
}

impl ConnectionMetadataExt {
    fn expected_authorization(&self) -> Option<ConnectionAuthorizationType> {
        match self {
            ConnectionMetadataExt::Outbound {
                expected_authorization,
                ..
            } => Some(expected_authorization.clone()),
            _ => None,
        }
    }

    fn local_authorization(&self) -> ConnectionAuthorizationType {
        match self {
            ConnectionMetadataExt::Outbound {
                local_authorization,
                ..
            } => local_authorization.clone(),
            ConnectionMetadataExt::Inbound {
                local_authorization,
                ..
            } => local_authorization.clone(),
        }
    }
}

/// Information required to request an outboudn connection
struct OutboundConnection {
    endpoint: String,
    connection_id: String,
    expected_authorization: Option<ConnectionAuthorizationType>,
    local_authorization: Option<ConnectionAuthorizationType>,
}

/// Struct describing the connection manager's internal state and handling
/// requests sent to the connection manager by its Connectors. Connection state
/// is responsible for adding, removing, and authorizing connections.
struct ConnectionManagerState<T, U>
where
    T: ConnectionMatrixLifeCycle,
    U: ConnectionMatrixSender,
{
    connections: HashMap<String, ConnectionMetadata>,
    life_cycle: T,
    matrix_sender: U,
    transport: Box<dyn Transport>,
    maximum_retry_frequency: u64,
}

impl<T, U> ConnectionManagerState<T, U>
where
    T: ConnectionMatrixLifeCycle,
    U: ConnectionMatrixSender,
{
    fn new(
        life_cycle: T,
        matrix_sender: U,
        transport: Box<dyn Transport + Send>,
        maximum_retry_frequency: u64,
    ) -> Self {
        Self {
            life_cycle,
            matrix_sender,
            transport,
            connections: HashMap::new(),
            maximum_retry_frequency,
        }
    }

    /// Adds a new connection as an inbound connection.
    fn add_inbound_connection(
        &mut self,
        connection: Box<dyn Connection>,
        reply_sender: Sender<Result<(), ConnectionManagerError>>,
        internal_sender: Sender<CmMessage>,
        authorizer: &dyn Authorizer,
    ) {
        let endpoint = connection.remote_endpoint();
        let id = Uuid::new_v4().to_string();

        // add the connection to the authorization pool.
        let auth_endpoint = endpoint;
        if let Err(err) = authorizer.authorize_connection(
            id,
            connection,
            Box::new(move |auth_result| {
                internal_sender
                    .send(CmMessage::AuthResult(AuthResult::Inbound {
                        endpoint: auth_endpoint.clone(),
                        auth_result,
                    }))
                    .map_err(Box::from)
            }),
            None,
            None,
        ) {
            if reply_sender
                .send(Err(ConnectionManagerError::connection_creation_error(
                    &err.to_string(),
                )))
                .is_err()
            {
                warn!("connector dropped before receiving result of add connection");
            }
        } else if reply_sender.send(Ok(())).is_err() {
            warn!("connector dropped before receiving result of add connection");
        }
    }

    /// Adds a new outbound connection.
    fn add_outbound_connection(
        &mut self,
        outbound: OutboundConnection,
        reply_sender: Sender<Result<(), ConnectionManagerError>>,
        internal_sender: Sender<CmMessage>,
        authorizer: &dyn Authorizer,
        subscribers: &mut SubscriberMap,
    ) {
        if let Some(connection) = self.connections.get(&outbound.connection_id) {
            let identity = connection.identity().clone();
            // if endpoints haven't changed, the connection is either connected or reconnecting
            if outbound.endpoint == connection.endpoint {
                // if this connection not reconnecting or disconnected, send Connected
                // notification.
                match connection.extended_metadata {
                    ConnectionMetadataExt::Outbound {
                        ref reconnecting,
                        ref local_authorization,
                        ..
                    } => {
                        if !reconnecting {
                            subscribers.broadcast(ConnectionManagerNotification::Connected {
                                endpoint: outbound.endpoint.to_string(),
                                connection_id: outbound.connection_id.to_string(),
                                identity,
                                local_identity: local_authorization.clone(),
                            });
                        }
                    }
                    ConnectionMetadataExt::Inbound {
                        ref disconnected,
                        ref local_authorization,
                    } => {
                        if !disconnected {
                            subscribers.broadcast(ConnectionManagerNotification::Connected {
                                endpoint: outbound.endpoint.to_string(),
                                connection_id: outbound.connection_id.to_string(),
                                identity,
                                local_identity: local_authorization.clone(),
                            });
                        }
                    }
                }

                if reply_sender.send(Ok(())).is_err() {
                    warn!("connector dropped before receiving result of add connection");
                }
                return;
            }
        }

        // The connection id is either new or the associated endpoint has changed
        match self.transport.connect(&outbound.endpoint) {
            Ok(connection) => {
                // add the connection to the authorization pool.
                let auth_endpoint = outbound.endpoint.to_string();
                if let Err(err) = authorizer.authorize_connection(
                    outbound.connection_id,
                    connection,
                    Box::new(move |auth_result| {
                        internal_sender
                            .send(CmMessage::AuthResult(AuthResult::Outbound {
                                endpoint: auth_endpoint.clone(),
                                auth_result,
                            }))
                            .map_err(Box::from)
                    }),
                    outbound.expected_authorization.clone(),
                    outbound.local_authorization.clone(),
                ) {
                    if reply_sender
                        .send(Err(ConnectionManagerError::connection_creation_error(
                            &err.to_string(),
                        )))
                        .is_err()
                    {
                        warn!("connector dropped before receiving result of add connection");
                    }
                } else if reply_sender.send(Ok(())).is_err() {
                    warn!("connector dropped before receiving result of add connection");
                }
            }
            Err(err) => {
                let connection_error = match err {
                    ConnectError::IoError(io_err) => {
                        ConnectionManagerError::connection_creation_error_with_io(
                            &format!(
                                "Unable to connect to {} ({})",
                                outbound.endpoint, outbound.connection_id
                            ),
                            io_err.kind(),
                        )
                    }
                    _ => ConnectionManagerError::connection_creation_error(&err.to_string()),
                };
                if reply_sender.send(Err(connection_error)).is_err() {
                    warn!("connector dropped before receiving result of add connection");
                }
            }
        }
    }

    /// Adds outbound connection to matrix life cycle after the connection has
    /// been authorized. These connections cannot be reconnected when dropped
    /// or lost.
    ///
    /// # Returns
    ///
    /// A string representing the Connection ID.
    ///
    /// # Errors
    ///
    /// Returns a connection manager error if the connection is unauthorized or
    /// if the life cycle fails to add the connection.
    fn on_outbound_authorization_complete(
        &mut self,
        endpoint: String,
        auth_result: AuthorizationResult,
        subscribers: &mut SubscriberMap,
    ) {
        match auth_result {
            AuthorizationResult::Authorized {
                connection_id,
                connection,
                identity,
                expected_authorization,
                local_authorization,
            } => {
                if let Err(err) = self
                    .life_cycle
                    .add(connection, connection_id.clone())
                    .map_err(|err| {
                        ConnectionManagerError::connection_creation_error(&err.to_string())
                    })
                {
                    subscribers.broadcast(ConnectionManagerNotification::FatalConnectionError {
                        endpoint,
                        connection_id,
                        error: err,
                    });

                    return;
                }

                self.connections.insert(
                    connection_id.to_string(),
                    ConnectionMetadata {
                        connection_id: connection_id.to_string(),
                        identity: identity.clone(),
                        endpoint: endpoint.clone(),
                        extended_metadata: ConnectionMetadataExt::Outbound {
                            reconnecting: false,
                            retry_frequency: INITIAL_RETRY_FREQUENCY,
                            last_connection_attempt: Instant::now(),
                            reconnection_attempts: 0,
                            expected_authorization,
                            local_authorization: local_authorization.clone(),
                        },
                    },
                );

                subscribers.broadcast(ConnectionManagerNotification::Connected {
                    endpoint,
                    connection_id,
                    identity,
                    local_identity: local_authorization,
                });
            }
            AuthorizationResult::Unauthorized { connection_id, .. } => {
                if self.connections.remove(&connection_id).is_some() {
                    warn!(
                        "Reconnecting connection {} ({}) failed authorization",
                        endpoint, connection_id
                    );
                }
                // If the connection is unauthorized, notify subscriber this is a bad connection
                // and will not be added.
                subscribers.broadcast(ConnectionManagerNotification::FatalConnectionError {
                    endpoint,
                    connection_id: connection_id.clone(),
                    error: ConnectionManagerError::Unauthorized(connection_id),
                });
            }
        }
    }

    /// Adds inbound connection to matrix life cycle after it has been authorized.
    ///
    /// # Errors
    ///
    /// Returns a connection manager error if the connection is unauthorized or
    /// if the life cycle fails to add the connection.
    fn on_inbound_authorization_complete(
        &mut self,
        endpoint: String,
        auth_result: AuthorizationResult,
        subscribers: &mut SubscriberMap,
    ) {
        match auth_result {
            AuthorizationResult::Authorized {
                connection_id,
                connection,
                identity,
                local_authorization,
                ..
            } => {
                if let Err(err) = self
                    .life_cycle
                    .add(connection, connection_id.clone())
                    .map_err(|err| {
                        ConnectionManagerError::connection_creation_error(&err.to_string())
                    })
                {
                    subscribers.broadcast(ConnectionManagerNotification::FatalConnectionError {
                        endpoint,
                        connection_id,
                        error: err,
                    });
                    return;
                }

                self.connections.insert(
                    connection_id.clone(),
                    ConnectionMetadata {
                        connection_id: connection_id.clone(),
                        endpoint: endpoint.clone(),
                        identity: identity.clone(),
                        extended_metadata: ConnectionMetadataExt::Inbound {
                            disconnected: false,
                            local_authorization: local_authorization.clone(),
                        },
                    },
                );

                subscribers.broadcast(ConnectionManagerNotification::InboundConnection {
                    endpoint,
                    connection_id,
                    identity,
                    local_identity: local_authorization,
                });
            }
            AuthorizationResult::Unauthorized { connection_id, .. } => {
                // If the connection is unauthorized, notify subscriber this is a bad connection
                // and will not be added.
                subscribers.broadcast(ConnectionManagerNotification::FatalConnectionError {
                    endpoint,
                    connection_id: connection_id.clone(),
                    error: ConnectionManagerError::Unauthorized(connection_id),
                });
            }
        }
    }

    /// Removes connection from state.
    ///
    /// # Returns
    ///
    /// Returns metadata for the connection if available.
    ///
    /// # Errors
    ///
    /// ConnectionManagerError if the connection cannot be removed from
    /// the matrix life cycle.
    fn remove_connection(
        &mut self,
        endpoint: &str,
        connection_id: &str,
    ) -> Result<Option<ConnectionMetadata>, ConnectionManagerError> {
        debug!("Remove connection {} ({})", endpoint, connection_id);
        let meta = if let Some(meta) = self.connections.get_mut(connection_id) {
            meta.clone()
        } else {
            return Ok(None);
        };

        self.connections.remove(connection_id);
        // remove mesh id, this may happen before reconnection is attempted
        self.life_cycle
            .remove(meta.connection_id())
            .map_err(|err| {
                ConnectionManagerError::ConnectionRemovalError(format!(
                    "Cannot remove connection {} ({}) from life cycle: {}",
                    endpoint,
                    meta.connection_id(),
                    err
                ))
            })?;

        Ok(Some(meta))
    }

    /// Handles reconnection operation.
    ///
    /// # Errors
    ///
    /// Returns ConnectionManagerError if reconnection operation fails due to
    /// an error caused by the matrix life cycle.
    fn reconnect(
        &mut self,
        endpoint: &str,
        connection_id: &str,
        subscribers: &mut SubscriberMap,
        authorizer: &dyn Authorizer,
        internal_sender: Sender<CmMessage>,
    ) -> Result<(), ConnectionManagerError> {
        let mut meta = if let Some(meta) = self.connections.get_mut(connection_id) {
            meta.clone()
        } else {
            return Err(ConnectionManagerError::ConnectionRemovalError(
                "Cannot reconnect to endpoint without metadata".into(),
            ));
        };

        if !meta.is_outbound() {
            // Do not attempt to reconnect inbound connections.
            return Ok(());
        }

        if let Ok(connection) = self.transport.connect(endpoint) {
            // remove old mesh id, this may happen before reconnection is attempted
            self.life_cycle
                .remove(meta.connection_id())
                .map_err(|err| {
                    ConnectionManagerError::ConnectionRemovalError(format!(
                        "Cannot remove connection {} ({}) from life cycle: {}",
                        endpoint,
                        meta.connection_id(),
                        err
                    ))
                })?;

            let auth_endpoint = endpoint.to_string();
            if let Err(err) = authorizer.authorize_connection(
                meta.connection_id().into(),
                connection,
                Box::new(move |auth_result| {
                    internal_sender
                        .send(CmMessage::AuthResult(AuthResult::Outbound {
                            endpoint: auth_endpoint.clone(),
                            auth_result,
                        }))
                        .map_err(Box::from)
                }),
                meta.extended_metadata.expected_authorization(),
                Some(meta.extended_metadata.local_authorization()),
            ) {
                error!(
                    "Error authorizing {} ({}): {}",
                    endpoint,
                    meta.connection_id(),
                    err
                );
            }
        } else {
            let reconnection_attempts = match meta.extended_metadata {
                ConnectionMetadataExt::Outbound {
                    ref mut reconnecting,
                    ref mut retry_frequency,
                    ref mut last_connection_attempt,
                    ref mut reconnection_attempts,
                    ..
                } => {
                    *reconnecting = true;
                    *retry_frequency = min(*retry_frequency * 2, self.maximum_retry_frequency);
                    *last_connection_attempt = Instant::now();
                    *reconnection_attempts += 1;

                    *reconnection_attempts
                }
                // We checked earlier that this was an outbound connection
                _ => unreachable!(),
            };
            let identity = meta.identity.clone();
            self.connections.insert(connection_id.to_string(), meta);

            // Notify subscribers of reconnection failure
            subscribers.broadcast(ConnectionManagerNotification::NonFatalConnectionError {
                endpoint: endpoint.to_string(),
                attempts: reconnection_attempts,
                identity,
                connection_id: connection_id.to_string(),
            });
        }
        Ok(())
    }

    fn connection_metadata(&self) -> &HashMap<String, ConnectionMetadata> {
        &self.connections
    }

    fn connection_metadata_mut(&mut self) -> &mut HashMap<String, ConnectionMetadata> {
        &mut self.connections
    }

    fn matrix_sender(&self) -> U {
        self.matrix_sender.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::mpsc;
    #[cfg(feature = "challenge-authorization")]
    use std::sync::{Arc, Mutex};

    #[cfg(feature = "challenge-authorization")]
    use cylinder::{secp256k1::Secp256k1Context, Context, Signer};
    use cylinder::{PublicKey, Signature, VerificationError, Verifier, VerifierFactory};
    use protobuf::Message;

    use crate::mesh::Mesh;
    use crate::network::auth::tests::negotiation_connection_auth;
    use crate::network::auth::AuthorizationManager;
    use crate::protos::network::{NetworkMessage, NetworkMessageType};
    use crate::threading::lifecycle::ShutdownHandle;
    use crate::transport::inproc::InprocTransport;
    use crate::transport::socket::TcpTransport;

    #[test]
    fn test_connection_manager_startup_and_shutdown() {
        let mut transport = Box::new(InprocTransport::default());
        transport.listen("inproc://test").unwrap();
        let mesh = Mesh::new(512, 128);

        let mut cm = ConnectionManager::builder()
            .with_authorizer(Box::new(NoopAuthorizer::new("test_identity")))
            .with_matrix_life_cycle(mesh.get_life_cycle())
            .with_matrix_sender(mesh.get_sender())
            .with_transport(transport)
            .start()
            .expect("Unable to start Connection Manager");

        cm.signal_shutdown();
        cm.wait_for_shutdown()
            .expect("Unable to shutdown connection manager");
    }

    #[test]
    fn test_add_connection_request() {
        let mut transport = Box::new(InprocTransport::default());
        let mut listener = transport.listen("inproc://test").unwrap();

        thread::spawn(move || {
            listener.accept().unwrap();
        });

        let mesh = Mesh::new(512, 128);
        let mut cm = ConnectionManager::builder()
            .with_authorizer(Box::new(NoopAuthorizer::new("test_identity")))
            .with_matrix_life_cycle(mesh.get_life_cycle())
            .with_matrix_sender(mesh.get_sender())
            .with_transport(transport)
            .start()
            .expect("Unable to start Connection Manager");

        let connector = cm.connector();

        connector
            .request_connection("inproc://test", "test_id", None, None)
            .expect("A connection could not be created");

        cm.signal_shutdown();
        cm.wait_for_shutdown()
            .expect("Unable to shutdown connection manager");
    }

    /// Test that adding the same connection twice is an idempotent operation
    #[test]
    fn test_mutiple_add_connection_requests() {
        let mut transport = Box::new(InprocTransport::default());
        let mut listener = transport.listen("inproc://test").unwrap();

        thread::spawn(move || {
            listener.accept().unwrap();
        });

        let mesh = Mesh::new(512, 128);
        let mut cm = ConnectionManager::builder()
            .with_authorizer(Box::new(NoopAuthorizer::new("test_identity")))
            .with_matrix_life_cycle(mesh.get_life_cycle())
            .with_matrix_sender(mesh.get_sender())
            .with_transport(transport)
            .start()
            .expect("Unable to start Connection Manager");

        let connector = cm.connector();

        connector
            .request_connection("inproc://test", "test_id", None, None)
            .expect("A connection could not be created");

        connector
            .request_connection("inproc://test", "test_id", None, None)
            .expect("A connection could not be re-requested");

        cm.signal_shutdown();
        cm.wait_for_shutdown()
            .expect("Unable to shutdown connection manager");
    }

    /// Test that heartbeats are correctly sent to inproc connections
    #[test]
    fn test_heartbeat_inproc() {
        let mut transport = Box::new(InprocTransport::default());
        let mut listener = transport.listen("inproc://test").unwrap();
        let mesh = Mesh::new(512, 128);
        let mesh_clone = mesh.clone();

        thread::spawn(move || {
            let conn = listener.accept().unwrap();
            mesh_clone.add(conn, "test_id".to_string()).unwrap();
        });

        let mut cm = ConnectionManager::builder()
            .with_authorizer(Box::new(NoopAuthorizer::new("test_identity")))
            .with_matrix_life_cycle(mesh.get_life_cycle())
            .with_matrix_sender(mesh.get_sender())
            .with_transport(transport)
            .start()
            .expect("Unable to start Connection Manager");

        let connector = cm.connector();

        connector
            .request_connection("inproc://test", "test_id", None, None)
            .expect("A connection could not be created");

        // Verify mesh received heartbeat

        let envelope = mesh.recv().unwrap();
        let heartbeat: NetworkMessage = Message::parse_from_bytes(&envelope.payload()).unwrap();
        assert_eq!(
            heartbeat.get_message_type(),
            NetworkMessageType::NETWORK_HEARTBEAT
        );

        cm.signal_shutdown();
        cm.wait_for_shutdown()
            .expect("Unable to shutdown connection manager");
    }

    /// Test that heartbeats are correctly sent to tcp connections
    #[test]
    fn test_heartbeat_raw_tcp() {
        let mut transport = Box::new(TcpTransport::default());
        let mut listener = transport.listen("tcp://localhost:0").unwrap();
        let endpoint = listener.endpoint();

        let mesh = Mesh::new(512, 128);

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut mesh = Mesh::new(512, 128);
            let conn = listener.accept().unwrap();
            mesh.add(conn, "test_id".to_string()).unwrap();

            negotiation_connection_auth(&mesh, "test_id", "some-peer");

            // Verify mesh received heartbeat

            let envelope = mesh.recv().unwrap();
            let heartbeat: NetworkMessage = Message::parse_from_bytes(&envelope.payload()).unwrap();
            assert_eq!(
                heartbeat.get_message_type(),
                NetworkMessageType::NETWORK_HEARTBEAT
            );

            tx.send(()).expect("Could not send completion signal");

            mesh.signal_shutdown();
            mesh.wait_for_shutdown().expect("Unable to shutdown mesh");
        });

        let auth_mgr = AuthorizationManager::new(
            "test_identity".into(),
            #[cfg(feature = "challenge-authorization")]
            vec![new_signer()],
            #[cfg(feature = "challenge-authorization")]
            Arc::new(Mutex::new(Box::new(NoopFactory))),
        )
        .expect("Unable to create authorization pool");
        let mut cm = ConnectionManager::builder()
            .with_authorizer(Box::new(auth_mgr.authorization_connector()))
            .with_matrix_life_cycle(mesh.get_life_cycle())
            .with_matrix_sender(mesh.get_sender())
            .with_transport(transport)
            .start()
            .expect("Unable to start Connection Manager");
        let connector = cm.connector();

        connector
            .request_connection(&endpoint, "test_id", None, None)
            .expect("A connection could not be created");

        let (sub_tx, sub_rx): (
            Sender<ConnectionManagerNotification>,
            mpsc::Receiver<ConnectionManagerNotification>,
        ) = channel();
        connector.subscribe(sub_tx).expect("Unable to respond.");

        // Validate that the connection completed authorization
        let notification = sub_rx.recv().expect("Cannot receive notification");
        assert!(
            notification
                == ConnectionManagerNotification::Connected {
                    endpoint: endpoint.clone(),
                    connection_id: "test_id".to_string(),
                    identity: ConnectionAuthorizationType::Trust {
                        identity: "some-peer".into()
                    },
                    local_identity: ConnectionAuthorizationType::Trust {
                        identity: "test_identity".into()
                    }
                }
        );

        // wait for completion
        rx.recv().expect("Did not receive completion signal");

        cm.signal_shutdown();
        cm.wait_for_shutdown()
            .expect("Unable to shutdown connection manager");
        auth_mgr.shutdown_and_await();
    }

    #[test]
    fn test_remove_connection() {
        let mut transport = Box::new(TcpTransport::default());
        let mut listener = transport.listen("tcp://localhost:0").unwrap();
        let endpoint = listener.endpoint();
        let mesh = Mesh::new(512, 128);

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut mesh = Mesh::new(512, 128);
            let conn = listener.accept().unwrap();
            mesh.add(conn, "test_id".to_string()).unwrap();
            negotiation_connection_auth(&mesh, "test_id", "some-peer");

            // wait for completion
            rx.recv().expect("Did not receive completion signal");

            mesh.signal_shutdown();
            mesh.wait_for_shutdown().expect("Unable to shutdown mesh");
        });

        let auth_mgr = AuthorizationManager::new(
            "test_identity".into(),
            #[cfg(feature = "challenge-authorization")]
            vec![new_signer()],
            #[cfg(feature = "challenge-authorization")]
            Arc::new(Mutex::new(Box::new(NoopFactory))),
        )
        .expect("Unable to create authorization pool");
        let mut cm = ConnectionManager::builder()
            .with_authorizer(Box::new(auth_mgr.authorization_connector()))
            .with_matrix_life_cycle(mesh.get_life_cycle())
            .with_matrix_sender(mesh.get_sender())
            .with_transport(transport)
            .start()
            .expect("Unable to start Connection Manager");
        let connector = cm.connector();

        let (sub_tx, sub_rx): (
            Sender<ConnectionManagerNotification>,
            mpsc::Receiver<ConnectionManagerNotification>,
        ) = channel();
        connector.subscribe(sub_tx).expect("Unable to respond.");

        connector
            .request_connection(&endpoint, "test_id", None, None)
            .expect("A connection could not be created");

        // Validate that the connection completed authorization
        let notification = sub_rx.recv().expect("Cannot receive notification");
        assert!(
            notification
                == ConnectionManagerNotification::Connected {
                    endpoint: endpoint.clone(),
                    connection_id: "test_id".to_string(),
                    identity: ConnectionAuthorizationType::Trust {
                        identity: "some-peer".into()
                    },
                    local_identity: ConnectionAuthorizationType::Trust {
                        identity: "test_identity".into()
                    }
                }
        );

        assert_eq!(
            vec![endpoint.clone()],
            connector
                .list_connections()
                .expect("Unable to list connections")
        );

        let endpoint_removed = connector
            .remove_connection(&endpoint, "test_id")
            .expect("Unable to remove connection");

        assert_eq!(Some(endpoint.clone()), endpoint_removed);

        assert!(connector
            .list_connections()
            .expect("Unable to list connections")
            .is_empty());

        tx.send(()).expect("Could not send completion signal");

        cm.signal_shutdown();
        cm.wait_for_shutdown()
            .expect("Unable to shutdown connection manager");
        auth_mgr.shutdown_and_await();
    }

    #[test]
    fn test_remove_nonexistent_connection() {
        let transport = Box::new(TcpTransport::default());
        let mesh = Mesh::new(512, 128);

        let mut cm = ConnectionManager::builder()
            .with_authorizer(Box::new(NoopAuthorizer::new("test_identity")))
            .with_matrix_life_cycle(mesh.get_life_cycle())
            .with_matrix_sender(mesh.get_sender())
            .with_transport(transport)
            .start()
            .expect("Unable to start Connection Manager");

        let connector = cm.connector();

        let endpoint_removed = connector
            .remove_connection("tcp://localhost:5000", "test_id")
            .expect("Unable to remove connection");

        assert_eq!(None, endpoint_removed);
        cm.signal_shutdown();
        cm.wait_for_shutdown()
            .expect("Unable to shutdown connection manager");
    }

    /// test_reconnect_raw_tcp
    ///
    /// Test that if a connection disconnects, the connection manager will detect the connection
    /// has disconnected by trying to send a heartbeat. Then connection manger will try to
    /// reconnect to the endpoint.
    #[test]
    fn test_reconnect_raw_tcp() {
        let mut transport = Box::new(TcpTransport::default());
        let mut listener = transport
            .listen("tcp://localhost:0")
            .expect("Cannot listen for connections");
        let endpoint = listener.endpoint();
        let mesh1 = Mesh::new(512, 128);

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            // accept incoming connection and add it to mesh2
            let mut mesh2 = Mesh::new(512, 128);
            let conn = listener.accept().expect("Cannot accept connection");
            mesh2
                .add(conn, "test_id".to_string())
                .expect("Cannot add connection to mesh");

            negotiation_connection_auth(&mesh2, "test_id", "some-peer");

            // Verify mesh received heartbeat
            let envelope = mesh2.recv().expect("Cannot receive message");
            let heartbeat: NetworkMessage = Message::parse_from_bytes(&envelope.payload())
                .expect("Cannot parse NetworkMessage");
            assert_eq!(
                heartbeat.get_message_type(),
                NetworkMessageType::NETWORK_HEARTBEAT
            );

            // remove connection to cause reconnection attempt
            let mut connection = mesh2
                .remove(&"test_id".to_string())
                .expect("Cannot remove connection from mesh");
            connection
                .disconnect()
                .expect("Connection failed to disconnect");

            // wait for reconnection attempt
            let conn = listener.accept().expect("Unable to accept connection");
            mesh2
                .add(conn, "test_id".to_string())
                .expect("Cannot add connection to mesh");
            negotiation_connection_auth(&mesh2, "test_id", "some-peer");

            // wait for completion
            rx.recv().expect("Did not receive completion signal");

            mesh2.signal_shutdown();
            mesh2.wait_for_shutdown().expect("Unable to shutdown mesh");
        });

        let auth_mgr = AuthorizationManager::new(
            "test_identity".into(),
            #[cfg(feature = "challenge-authorization")]
            vec![new_signer()],
            #[cfg(feature = "challenge-authorization")]
            Arc::new(Mutex::new(Box::new(NoopFactory))),
        )
        .expect("Unable to create authorization pool");
        let mut cm = ConnectionManager::builder()
            .with_authorizer(Box::new(auth_mgr.authorization_connector()))
            .with_matrix_life_cycle(mesh1.get_life_cycle())
            .with_matrix_sender(mesh1.get_sender())
            .with_transport(transport)
            .start()
            .expect("Unable to start Connection Manager");
        let connector = cm.connector();

        let (sub_tx, sub_rx): (
            Sender<ConnectionManagerNotification>,
            mpsc::Receiver<ConnectionManagerNotification>,
        ) = channel();
        connector.subscribe(sub_tx).expect("Unable to respond.");

        connector
            .request_connection(&endpoint, "test_id", None, None)
            .expect("A connection could not be created");

        // Validate that the connection completed authorization
        let notification = sub_rx.recv().expect("Cannot receive notification");
        assert!(
            notification
                == ConnectionManagerNotification::Connected {
                    endpoint: endpoint.clone(),
                    connection_id: "test_id".to_string(),
                    identity: ConnectionAuthorizationType::Trust {
                        identity: "some-peer".into()
                    },
                    local_identity: ConnectionAuthorizationType::Trust {
                        identity: "test_identity".into()
                    }
                }
        );

        let (subs_tx, subs_rx) = mpsc::channel();
        connector.subscribe(subs_tx).expect("Cannot subscribe");
        let mut subscriber = subs_rx.iter();

        // receive reconnecting attempt
        let reconnecting_notification: ConnectionManagerNotification = subscriber
            .next()
            .expect("Cannot get message from subscriber");

        assert!(
            reconnecting_notification
                == ConnectionManagerNotification::Disconnected {
                    endpoint: endpoint.clone(),
                    identity: ConnectionAuthorizationType::Trust {
                        identity: "some-peer".into()
                    },
                    connection_id: "test_id".into(),
                }
        );

        // receive successful reconnect attempt
        let reconnection_notification = subscriber
            .next()
            .expect("Cannot get message from subscriber");

        assert_eq!(
            reconnection_notification,
            ConnectionManagerNotification::Connected {
                endpoint: endpoint.clone(),
                connection_id: "test_id".to_string(),
                identity: ConnectionAuthorizationType::Trust {
                    identity: "some-peer".into()
                },
                local_identity: ConnectionAuthorizationType::Trust {
                    identity: "test_identity".into()
                }
            }
        );

        tx.send(()).expect("Could not send completion signal");

        cm.signal_shutdown();
        cm.wait_for_shutdown()
            .expect("Unable to shutdown connection manager");
        auth_mgr.shutdown_and_await();
    }

    /// Test that an inbound connection may be added to the connection manager
    /// This test does the following:
    /// 1. Add an inbound connection to a connection manager
    /// 2. Notify inbound listeners
    /// 3. The connection can be removed by its reported remote endpoint
    #[test]
    fn test_inbound_connection() {
        let mut transport = InprocTransport::default();
        let mut listener = transport
            .listen("inproc://test_inbound_connection")
            .expect("Cannot listen for connections");

        let mesh = Mesh::new(512, 128);

        let (conn_tx, conn_rx) = mpsc::channel();

        let mut remote_transport = transport.clone();
        let jh = thread::spawn(move || {
            let _connection = remote_transport
                .connect("inproc://test_inbound_connection")
                .unwrap();

            // block until done
            conn_rx.recv().unwrap();
        });
        let mut cm = ConnectionManager::builder()
            .with_authorizer(Box::new(NoopAuthorizer::new("test_identity")))
            .with_matrix_life_cycle(mesh.get_life_cycle())
            .with_matrix_sender(mesh.get_sender())
            .with_transport(Box::new(transport))
            .start()
            .expect("Unable to start Connection Manager");

        let connector = cm.connector();

        let (subs_tx, subs_rx) = mpsc::channel();
        connector.subscribe(subs_tx).expect("Cannot get subscriber");

        let connection = listener.accept().unwrap();
        connector
            .add_inbound_connection(connection)
            .expect("Unable to add inbound connection");

        let notification = subs_rx
            .iter()
            .next()
            .expect("Cannot get message from subscriber");
        let connection_id_for_removal;
        if let ConnectionManagerNotification::InboundConnection {
            endpoint,
            connection_id,
            ..
        } = notification
        {
            connection_id_for_removal = connection_id.to_string();
            assert_eq!("inproc://test_inbound_connection", &endpoint);
        } else {
            panic!("Incorrect notification received: {:?}", notification);
        }

        let connection_endpoints = connector.list_connections().unwrap();
        assert_eq!(
            vec!["inproc://test_inbound_connection".to_string()],
            connection_endpoints
        );

        connector
            .remove_connection(
                "inproc://test_inbound_connection",
                &connection_id_for_removal,
            )
            .unwrap();
        let connection_endpoints = connector.list_connections().unwrap();
        assert!(connection_endpoints.is_empty());

        conn_tx.send(()).unwrap();
        jh.join().unwrap();

        cm.signal_shutdown();
        cm.wait_for_shutdown()
            .expect("Unable to shutdown connection manager");
    }

    /// Test that an inbound tcp connection can be add and removed from the network.o
    ///
    /// This connection requires negotiating the connection authorization handshake.
    #[test]
    fn test_inbound_tcp_connection() {
        let mut transport = Box::new(TcpTransport::default());
        let mut listener = transport
            .listen("tcp://localhost:0")
            .expect("Cannot listen for tcp connections");
        let endpoint = listener.endpoint();

        let mesh = Mesh::new(512, 128);
        let auth_mgr = AuthorizationManager::new(
            "test_identity".into(),
            #[cfg(feature = "challenge-authorization")]
            vec![new_signer()],
            #[cfg(feature = "challenge-authorization")]
            Arc::new(Mutex::new(Box::new(NoopFactory))),
        )
        .expect("Unable to create authorization pool");

        let (conn_tx, conn_rx) = mpsc::channel();
        let server_endpoint = endpoint.clone();
        let jh = thread::spawn(move || {
            let mut mesh = Mesh::new(512, 128);
            let mut transport = Box::new(TcpTransport::default());
            let connection = transport.connect(&server_endpoint).unwrap();

            mesh.add(connection, "test_id".into())
                .expect("Unable to add to remote mesh");

            negotiation_connection_auth(&mesh, "test_id", "inbound-identity");

            // block until done
            conn_rx.recv().unwrap();
            mesh.signal_shutdown();
            mesh.wait_for_shutdown().expect("Unable to shutdown mesh");
        });

        let mut cm = ConnectionManager::builder()
            .with_authorizer(Box::new(auth_mgr.authorization_connector()))
            .with_matrix_life_cycle(mesh.get_life_cycle())
            .with_matrix_sender(mesh.get_sender())
            .with_transport(transport)
            .start()
            .expect("Unable to start Connection Manager");
        let connector = cm.connector();

        let (subs_tx, subs_rx) = mpsc::channel();
        connector.subscribe(subs_tx).expect("Cannot get subscriber");

        let connection = listener.accept().unwrap();
        let remote_endpoint = connection.remote_endpoint();
        connector
            .add_inbound_connection(connection)
            .expect("Unable to add inbound connection");

        let notification = subs_rx
            .iter()
            .next()
            .expect("Cannot get message from subscriber");

        let connection_id_to_remove;

        if let ConnectionManagerNotification::InboundConnection {
            ref identity,
            connection_id,
            ..
        } = &notification
        {
            assert_eq!(
                identity,
                &ConnectionAuthorizationType::Trust {
                    identity: "inbound-identity".into()
                },
            );
            connection_id_to_remove = connection_id.to_string();
        } else {
            panic!(
                "Did not receive the correct notification: {:?}",
                notification
            );
        }

        let connection_endpoints = connector.list_connections().unwrap();
        assert_eq!(vec![remote_endpoint.clone()], connection_endpoints);

        connector
            .remove_connection(&remote_endpoint, &connection_id_to_remove)
            .unwrap();
        let connection_endpoints = connector.list_connections().unwrap();
        assert!(connection_endpoints.is_empty());

        conn_tx.send(()).unwrap();
        jh.join().unwrap();

        cm.signal_shutdown();
        cm.wait_for_shutdown()
            .expect("Unable to shutdown connection manager");
        auth_mgr.shutdown_and_await();
    }

    struct NoopAuthorizer {
        authorized_id: String,
    }

    impl NoopAuthorizer {
        fn new(id: &str) -> Self {
            Self {
                authorized_id: id.to_string(),
            }
        }
    }

    impl Authorizer for NoopAuthorizer {
        fn authorize_connection(
            &self,
            connection_id: String,
            connection: Box<dyn Connection>,
            callback: AuthorizerCallback,
            _expected_authorization: Option<ConnectionAuthorizationType>,
            _local_authorization: Option<ConnectionAuthorizationType>,
        ) -> Result<(), AuthorizerError> {
            (*callback)(AuthorizationResult::Authorized {
                connection_id,
                connection,
                identity: ConnectionAuthorizationType::Trust {
                    identity: self.authorized_id.clone(),
                },
                expected_authorization: ConnectionAuthorizationType::Trust {
                    identity: self.authorized_id.clone(),
                },
                local_authorization: ConnectionAuthorizationType::Trust {
                    identity: "test_identity".into(),
                },
            })
            .map_err(|err| AuthorizerError(format!("Unable to return result: {}", err)))
        }
    }

    struct NoopVerifier;

    impl Verifier for NoopVerifier {
        fn algorithm_name(&self) -> &str {
            unimplemented!()
        }

        fn verify(
            &self,
            _message: &[u8],
            _signature: &Signature,
            _public_key: &PublicKey,
        ) -> Result<bool, VerificationError> {
            unimplemented!()
        }
    }

    struct NoopFactory;

    impl VerifierFactory for NoopFactory {
        fn new_verifier(&self) -> Box<dyn Verifier> {
            Box::new(NoopVerifier)
        }
    }

    #[cfg(feature = "challenge-authorization")]
    fn new_signer() -> Box<dyn Signer> {
        let context = Secp256k1Context::new();
        let key = context.new_random_private_key();
        context.new_signer(key)
    }
}

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

//! Provides the "fetch node" operation for the `DieselRegistry`.

use diesel::prelude::*;

use crate::error::InvalidStateError;
use crate::registry::{
    diesel::{
        models::{NodeEndpointsModel, NodeKeysModel, NodeMetadataModel, NodesModel},
        schema::{
            splinter_nodes, splinter_nodes_endpoints, splinter_nodes_keys, splinter_nodes_metadata,
        },
    },
    Node, NodeBuilder, RegistryError,
};

use super::RegistryOperations;

pub(in crate::registry::diesel) trait RegistryFetchNodeOperation {
    fn get_node(&self, identity: &str) -> Result<Option<Node>, RegistryError>;
}

impl<'a, C> RegistryFetchNodeOperation for RegistryOperations<'a, C>
where
    C: diesel::Connection,
    String: diesel::deserialize::FromSql<diesel::sql_types::Text, C::Backend>,
{
    fn get_node(&self, identity: &str) -> Result<Option<Node>, RegistryError> {
        self.conn.transaction(|| {
            let node = splinter_nodes::table
                .find(identity)
                .first::<NodesModel>(self.conn)
                .optional()?;

            if let Some(node) = node {
                let endpoints = splinter_nodes_endpoints::table
                    .filter(splinter_nodes_endpoints::identity.eq(identity))
                    .load::<NodeEndpointsModel>(self.conn)?
                    .into_iter()
                    .map(|endpoint| endpoint.endpoint)
                    .collect::<Vec<_>>();
                let keys = splinter_nodes_keys::table
                    .filter(splinter_nodes_keys::identity.eq(identity))
                    .load::<NodeKeysModel>(self.conn)?
                    .into_iter()
                    .map(|key| key.key)
                    .collect::<Vec<_>>();
                let metadata = splinter_nodes_metadata::table
                    .filter(splinter_nodes_metadata::identity.eq(identity))
                    .load::<NodeMetadataModel>(self.conn)?;

                let mut builder = NodeBuilder::new(identity)
                    .with_display_name(node.display_name)
                    .with_endpoints(endpoints)
                    .with_keys(keys);
                for entry in metadata {
                    builder = builder.with_metadata(entry.key, entry.value);
                }
                Ok(Some(builder.build().map_err(|err| {
                    RegistryError::InvalidStateError(InvalidStateError::with_message(
                        err.to_string(),
                    ))
                })?))
            } else {
                Ok(None)
            }
        })
    }
}

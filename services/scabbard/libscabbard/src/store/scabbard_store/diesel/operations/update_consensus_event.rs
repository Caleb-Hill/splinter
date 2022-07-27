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

use std::convert::{TryFrom, TryInto};
use std::time::SystemTime;

use chrono::naive::NaiveDateTime;
use diesel::{prelude::*, update};
use splinter::error::{InternalError, InvalidStateError};
use splinter::service::FullyQualifiedServiceId;

use crate::store::scabbard_store::diesel::{
    models::{
        ConsensusTypeModel, ConsensusTypeModelMapping, ScabbardServiceModel,
        ServiceStatusTypeModel, ServiceStatusTypeModelMapping,
    },
    schema::{
        consensus_2pc_action, consensus_2pc_event, consensus_2pc_update_context_action,
        scabbard_service,
    },
};
use crate::store::scabbard_store::ScabbardStoreError;

use super::ScabbardStoreOperations;

const OPERATION_NAME: &str = "update_consensus_event";

pub(in crate::store::scabbard_store::diesel) trait UpdateEventOperation {
    fn update_consensus_event(
        &self,
        service_id: &FullyQualifiedServiceId,
        event_id: i64,
        executed_at: SystemTime,
        executed_epoch: u64,
    ) -> Result<(), ScabbardStoreError>;
}

impl<'a, C> UpdateEventOperation for ScabbardStoreOperations<'a, C>
where
    C: diesel::Connection,
    i64: diesel::deserialize::FromSql<diesel::sql_types::BigInt, C::Backend>,
    String: diesel::deserialize::FromSql<diesel::sql_types::Text, C::Backend>,
    NaiveDateTime: diesel::serialize::ToSql<diesel::sql_types::Timestamp, C::Backend>,
    <C as diesel::Connection>::Backend: diesel::types::HasSqlType<ServiceStatusTypeModelMapping>,
    ServiceStatusTypeModel: diesel::deserialize::FromSql<ServiceStatusTypeModelMapping, C::Backend>,
    <C as diesel::Connection>::Backend: diesel::types::HasSqlType<ConsensusTypeModelMapping>,
    ConsensusTypeModel: diesel::deserialize::FromSql<ConsensusTypeModelMapping, C::Backend>,
{
    fn update_consensus_event(
        &self,
        service_id: &FullyQualifiedServiceId,
        event_id: i64,
        executed_at: SystemTime,
        executed_epoch: u64,
    ) -> Result<(), ScabbardStoreError> {
        let update_executed_at = get_naive_date_time(executed_at)?;
        let update_executed_epoch: i64 = executed_epoch
            .try_into()
            .map_err(|err| InternalError::from_source(Box::new(err)))?;
        self.conn.transaction::<_, _, _>(|| {
            // check to see if a service with the given service_id exists
            scabbard_service::table
                .filter(
                    scabbard_service::circuit_id
                        .eq(service_id.circuit_id().to_string())
                        .and(scabbard_service::service_id.eq(service_id.service_id().to_string())),
                )
                .first::<ScabbardServiceModel>(self.conn)
                .optional()
                .map_err(|err| {
                    ScabbardStoreError::from_source_with_operation(err, OPERATION_NAME.to_string())
                })?
                .ok_or_else(|| {
                    ScabbardStoreError::InvalidState(InvalidStateError::with_message(String::from(
                        "Service does not exist",
                    )))
                })?;

            // get the action_id of the most recently executed update context action
            let update_context_action_id = consensus_2pc_update_context_action::table
                .inner_join(consensus_2pc_action::table.on(
                    consensus_2pc_update_context_action::action_id.eq(consensus_2pc_action::id),
                ))
                .filter(
                    consensus_2pc_action::circuit_id
                        .eq(service_id.circuit_id().to_string())
                        .and(
                            consensus_2pc_action::service_id
                                .eq(service_id.service_id().to_string()),
                        )
                        .and(consensus_2pc_action::executed_at.is_not_null()),
                )
                .order(consensus_2pc_action::executed_at.desc())
                .select(consensus_2pc_update_context_action::action_id)
                .first::<i64>(self.conn)
                .optional()
                .map_err(|err| {
                    ScabbardStoreError::from_source_with_operation(err, OPERATION_NAME.to_string())
                })?;

            update(consensus_2pc_event::table)
                .filter(
                    consensus_2pc_event::id.eq(event_id).and(
                        consensus_2pc_event::circuit_id
                            .eq(service_id.circuit_id().to_string())
                            .and(
                                consensus_2pc_event::service_id
                                    .eq(service_id.service_id().to_string()),
                            ),
                    ),
                )
                .set((
                    consensus_2pc_event::executed_at.eq(Some(update_executed_at)),
                    consensus_2pc_event::executed_epoch.eq(Some(update_executed_epoch)),
                    consensus_2pc_event::update_context_action_id.eq(update_context_action_id),
                ))
                .execute(self.conn)
                .map_err(|err| {
                    ScabbardStoreError::from_source_with_operation(err, OPERATION_NAME.to_string())
                })?;
            Ok(())
        })
    }
}

fn get_naive_date_time(time: SystemTime) -> Result<NaiveDateTime, InternalError> {
    let duration = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|err| InternalError::from_source(Box::new(err)))?;
    let seconds = i64::try_from(duration.as_secs())
        .map_err(|err| InternalError::from_source(Box::new(err)))?;
    Ok(NaiveDateTime::from_timestamp(
        seconds,
        duration.subsec_millis() * 1_000_000,
    ))
}

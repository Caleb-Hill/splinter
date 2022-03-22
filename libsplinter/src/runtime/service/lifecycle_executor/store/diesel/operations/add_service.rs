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

//! Provides the "add service" operation for the `DieselLifecycleStore`.

use std::convert::TryFrom;

use diesel::{dsl::insert_into, prelude::*};

use crate::error::{ConstraintViolationError, ConstraintViolationType};
use crate::runtime::service::lifecycle_executor::store::{
    diesel::{
        models::{ServiceLifecycleArgumentModel, ServiceLifecycleStatusModel},
        schema::{service_lifecycle_argument, service_lifecycle_status},
    },
    error::LifecycleStoreError,
    LifecycleService,
};

use super::LifecycleStoreOperations;

pub(in crate::runtime::service::lifecycle_executor::store::diesel) trait LifecycleStoreAddServiceOperation
{
    fn add_service(&self, service: LifecycleService) -> Result<(), LifecycleStoreError>;
}

#[cfg(feature = "postgres")]
impl<'a> LifecycleStoreAddServiceOperation
    for LifecycleStoreOperations<'a, diesel::pg::PgConnection>
{
    fn add_service(&self, service: LifecycleService) -> Result<(), LifecycleStoreError> {
        self.conn.transaction::<(), _, _>(|| {
            if service_lifecycle_status::table
                .filter(
                    service_lifecycle_status::circuit_id
                        .eq(service.service_id().circuit_id().as_str()),
                )
                .filter(
                    service_lifecycle_status::service_id
                        .eq(service.service_id().service_id().as_str()),
                )
                .first::<ServiceLifecycleStatusModel>(self.conn)
                .optional()?
                .is_some()
            {
                return Err(LifecycleStoreError::ConstraintViolation(
                    ConstraintViolationError::with_violation_type(ConstraintViolationType::Unique),
                ));
            }

            // Create a `Model` from the `LifecycleService` to add to database
            let service_model = ServiceLifecycleStatusModel::from(&service);
            insert_into(service_lifecycle_status::table)
                .values(service_model)
                .execute(self.conn)?;

            let service_arguments = Vec::<ServiceLifecycleArgumentModel>::try_from(&service)?;
            insert_into(service_lifecycle_argument::table)
                .values(&service_arguments)
                .execute(self.conn)?;

            Ok(())
        })
    }
}

#[cfg(feature = "sqlite")]
impl<'a> LifecycleStoreAddServiceOperation
    for LifecycleStoreOperations<'a, diesel::sqlite::SqliteConnection>
{
    fn add_service(&self, service: LifecycleService) -> Result<(), LifecycleStoreError> {
        self.conn.transaction::<(), _, _>(|| {
            if service_lifecycle_status::table
                .filter(
                    service_lifecycle_status::circuit_id
                        .eq(service.service_id().circuit_id().as_str()),
                )
                .filter(
                    service_lifecycle_status::service_id
                        .eq(service.service_id().service_id().as_str()),
                )
                .first::<ServiceLifecycleStatusModel>(self.conn)
                .optional()?
                .is_some()
            {
                return Err(LifecycleStoreError::ConstraintViolation(
                    ConstraintViolationError::with_violation_type(ConstraintViolationType::Unique),
                ));
            }

            // Create a `Model` from the `LifecycleService` to add to database
            let service_model = ServiceLifecycleStatusModel::from(&service);
            insert_into(service_lifecycle_status::table)
                .values(service_model)
                .execute(self.conn)?;

            let service_arguments = Vec::<ServiceLifecycleArgumentModel>::try_from(&service)?;
            insert_into(service_lifecycle_argument::table)
                .values(&service_arguments)
                .execute(self.conn)?;

            Ok(())
        })
    }
}

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

use diesel::{dsl::delete, prelude::*};

use crate::rest_api::auth::authorization::rbac::store::{
    diesel::schema::{rbac_role_permissions, rbac_roles},
    RoleBasedAuthorizationStoreError,
};

use super::RoleBasedAuthorizationStoreOperations;

pub trait RoleBasedAuthorizationStoreRemoveRole {
    fn remove_role(&self, role_id: &str) -> Result<(), RoleBasedAuthorizationStoreError>;
}

impl<'a, C> RoleBasedAuthorizationStoreRemoveRole for RoleBasedAuthorizationStoreOperations<'a, C>
where
    C: diesel::Connection,
    String: diesel::deserialize::FromSql<diesel::sql_types::Text, C::Backend>,
{
    fn remove_role(&self, role_id: &str) -> Result<(), RoleBasedAuthorizationStoreError> {
        self.conn.transaction::<_, _, _>(|| {
            delete(rbac_role_permissions::table.filter(rbac_role_permissions::role_id.eq(role_id)))
                .execute(self.conn)?;

            delete(rbac_roles::table.filter(rbac_roles::id.eq(role_id))).execute(self.conn)?;

            Ok(())
        })
    }
}

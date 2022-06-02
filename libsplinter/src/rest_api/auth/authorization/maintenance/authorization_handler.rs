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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::InternalError;
#[cfg(feature = "authorization-handler-rbac")]
use crate::rbac::store::{Identity as RBACIdentity, RoleBasedAuthorizationStore, ADMIN_ROLE_ID};
use crate::rest_api::auth::identity::Identity;

use super::{AuthorizationHandler, AuthorizationHandlerResult};

/// An authorization handler that allows write permissions to be temporarily revoked
///
/// For the purposes of this authorization handler, a write permission is any permission whose ID
/// does not end in ".read". Any permission whose ID ends with ".read" will be ignored by this
/// authorization handler (checking those permission will result in
/// [`AuthorizationHandlerResult::Continue`]).
///
/// For all non-read permission checks, this authoirzation handler will decide to deny or pass based
/// on whether or not maintenance mode is enabled. If maintenance mode is enabled, checks for
/// non-read permission will always result in a [`AuthorizationHandlerResult::Deny`] result; if
/// disabled, all permission checks will always result in a [`AuthorizationHandlerResult::Continue`]
/// result.
#[derive(Clone, Default)]
pub struct MaintenanceModeAuthorizationHandler {
    maintenance_mode: Arc<AtomicBool>,
    #[cfg(feature = "authorization-handler-rbac")]
    rbac_store: Option<Box<dyn RoleBasedAuthorizationStore>>,
}

impl MaintenanceModeAuthorizationHandler {
    /// Constructs a new `MaintenanceModeAuthorizationHandler`
    ///
    /// # Arguments
    ///
    /// * `rbac_store` - If provided, this will be used to allow identities with the "admin" role
    ///   defined in the RBAC store to perform write operations even with maintenance mode enabled
    #[cfg(feature = "authorization-handler-rbac")]
    pub fn new(rbac_store: Option<Box<dyn RoleBasedAuthorizationStore>>) -> Self {
        Self {
            rbac_store,
            ..Default::default()
        }
    }

    /// Returns whether or not maintenance mode is enabled
    pub fn is_maintenance_mode_enabled(&self) -> bool {
        self.maintenance_mode.load(Ordering::Relaxed)
    }

    /// Sets whether or not maintenance mode is enabled
    pub fn set_maintenance_mode(&self, maintenance_mode: bool) {
        self.maintenance_mode
            .store(maintenance_mode, Ordering::Relaxed);
    }
}

impl AuthorizationHandler for MaintenanceModeAuthorizationHandler {
    fn has_permission(
        &self,
        // Allow `unused_variables` in case `authorization-handler-rbac` feature is not enabled
        #[allow(unused_variables)] identity: &Identity,
        permission_id: &str,
    ) -> Result<AuthorizationHandlerResult, InternalError> {
        if !permission_id.ends_with(".read") && self.maintenance_mode.load(Ordering::Relaxed) {
            // Check if the client has the "admin" role, in which case they're not denied permission
            #[cfg(feature = "authorization-handler-rbac")]
            {
                let is_admin = self
                    .rbac_store
                    .as_ref()
                    .and_then(|store| {
                        let rbac_identity: Option<RBACIdentity> = identity.into();
                        Some(
                            store
                                .get_assignment(&rbac_identity?)
                                .ok()??
                                .roles()
                                .iter()
                                .any(|role| role == ADMIN_ROLE_ID),
                        )
                    })
                    .unwrap_or(false);
                if is_admin {
                    return Ok(AuthorizationHandlerResult::Continue);
                }
            }
            Ok(AuthorizationHandlerResult::Deny)
        } else {
            Ok(AuthorizationHandlerResult::Continue)
        }
    }

    fn clone_box(&self) -> Box<dyn AuthorizationHandler> {
        Box::new(self.clone())
    }
}

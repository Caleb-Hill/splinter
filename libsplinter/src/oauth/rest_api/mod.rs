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

//! OAuth REST API endpoints

#[cfg(feature = "rest-api-actix-web-1")]
mod actix;
mod resources;

use crate::biome::OAuthUserSessionStore;
use crate::rest_api::actix_web_1::{Resource, RestResourceProvider};
#[cfg(all(feature = "authorization", feature = "rest-api-actix-web-1",))]
use crate::rest_api::auth::authorization::Permission;

#[cfg(feature = "biome-profile")]
use crate::biome::UserProfileStore;

use super::OAuthClient;

#[cfg(all(feature = "authorization", feature = "rest-api-actix-web-1",))]
const OAUTH_USER_READ_PERMISSION: Permission = Permission::Check {
    permission_id: "oauth.users.read",
    permission_display_name: "OAuth Users read",
    permission_description: "Allows the client to read OAuth users",
};

/// Provides the REST API [Resource](../../../rest_api/struct.Resource.html) definitions for OAuth
/// endpoints. The following endpoints are provided:
///
/// * `GET /oauth/login` - Get the URL for requesting authorization from the provider
/// * `GET /oauth/callback` - Receive the authorization code from the provider
/// * `GET /oauth/logout` - Remove the user's access and refresh tokens
///
/// These endpoints are only available if the following REST API backend feature is enabled:
///
/// * `rest-api-actix-web-1`
#[derive(Clone)]
pub struct OAuthResourceProvider {
    client: OAuthClient,
    oauth_user_session_store: Box<dyn OAuthUserSessionStore>,
    #[cfg(feature = "biome-profile")]
    user_profile_store: Box<dyn UserProfileStore>,
}

impl OAuthResourceProvider {
    /// Creates a new `OAuthResourceProvider`
    pub fn new(
        client: OAuthClient,
        oauth_user_session_store: Box<dyn OAuthUserSessionStore>,
        #[cfg(feature = "biome-profile")] user_profile_store: Box<dyn UserProfileStore>,
    ) -> Self {
        Self {
            client,
            oauth_user_session_store,
            #[cfg(feature = "biome-profile")]
            user_profile_store,
        }
    }
}

/// `OAuthResourceProvider` provides the following endpoints as REST API resources:
///
/// * `GET /oauth/login` - Get the URL for requesting authorization from the provider
/// * `GET /oauth/callback` - Receive the authorization code from the provider
/// * `GET /oauth/logout` - Remove the user's access and refresh tokens
/// * `GET` /oauth/users` - Get a list of the OAuth users
///
/// These endpoints are only available if the following REST API backend feature is enabled:
///
/// * `rest-api-actix-web-1`
impl RestResourceProvider for OAuthResourceProvider {
    fn resources(&self) -> Vec<Resource> {
        // Allowing unused_mut because resources must be mutable if feature `rest-api-actix-web-1` is
        // enabled
        #[allow(unused_mut)]
        let mut resources = Vec::new();

        #[cfg(feature = "rest-api-actix-web-1")]
        {
            resources.append(&mut vec![
                actix::login::make_login_route(self.client.clone()),
                actix::callback::make_callback_route(
                    self.client.clone(),
                    self.oauth_user_session_store.clone(),
                    #[cfg(feature = "biome-profile")]
                    self.user_profile_store.clone(),
                ),
                actix::logout::make_logout_route(self.oauth_user_session_store.clone()),
            ]);
            resources.append(&mut vec![
                actix::list_users::make_oauth_list_users_resource(
                    self.oauth_user_session_store.clone(),
                ),
            ]);
        }

        resources
    }
}

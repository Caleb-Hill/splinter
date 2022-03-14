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

#[cfg(feature = "authorization")]
use splinter::rest_api::auth::authorization::AuthorizationHandler;
use splinter::rest_api::auth::identity::IdentityProvider;
use splinter::rest_api::{BindConfig, RestApiServerError};
#[cfg(feature = "store-factory")]
use splinter::store::StoreFactory;

use crate::api::RestApi;
use crate::resource_provider::ResourceProvider;

/// A configured REST API which may best started with `run` function.
pub struct RunnableRestApi {
    pub(super) resource_providers: Vec<Box<dyn ResourceProvider>>,
    pub(super) bind: BindConfig,
    #[cfg(feature = "store-factory")]
    pub(super) store_factory: Option<Box<dyn StoreFactory + Send>>,
    pub(super) identity_providers: Vec<Box<dyn IdentityProvider>>,
    #[cfg(feature = "authorization")]
    pub(super) authorization_handlers: Vec<Box<dyn AuthorizationHandler>>,
}

impl RunnableRestApi {
    /// Start the REST API and finish any necessary setup such as binding to ports, adding resource
    /// endpoints, etc.
    pub fn run(self) -> Result<RestApi, RestApiServerError> {
        let RunnableRestApi {
            resource_providers,
            bind,
            identity_providers,
            #[cfg(feature = "authorization")]
            authorization_handlers,
            #[cfg(feature = "store-factory")]
            store_factory,
        } = self;

        let (bind_url, acceptor_opt) = match bind {
            #[cfg(feature = "https-bind")]
            BindConfig::Https {
                bind,
                cert_path,
                key_path,
            } => {
                let mut acceptor =
                    openssl::ssl::SslAcceptor::mozilla_modern(openssl::ssl::SslMethod::tls())?;
                acceptor.set_private_key_file(key_path, openssl::ssl::SslFiletype::PEM)?;
                acceptor.set_certificate_chain_file(&cert_path)?;
                acceptor.check_private_key()?;
                (bind, Some(acceptor))
            }
            BindConfig::Http(url) => (url, None),
        };
        RestApi::new(
            bind_url,
            acceptor_opt,
            resource_providers,
            identity_providers,
            #[cfg(feature = "store-factory")]
            store_factory,
            #[cfg(feature = "authorization")]
            authorization_handlers,
        )
    }
}

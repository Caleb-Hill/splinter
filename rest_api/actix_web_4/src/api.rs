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

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
#[cfg(feature = "authorization")]
use std::sync::RwLock;
use std::sync::{Arc, Mutex};

use actix_web::dev::ServerHandle;
use actix_web::rt::System;
use actix_web::{middleware, App, HttpServer};
use futures::executor::block_on;
use log::{error, info};
use openssl::ssl::SslAcceptorBuilder;

use splinter::error::InternalError;
#[cfg(feature = "authorization")]
use splinter::rest_api::auth::authorization::{AuthorizationHandler, PermissionMap};
use splinter::rest_api::auth::identity::IdentityProvider;
use splinter::rest_api::RestApiServerError;
#[cfg(feature = "store-factory")]
use splinter::store::StoreFactory;

use crate::resource_provider::ResourceProvider;

/// A running instance of the REST API.
pub struct RestApi {
    bind_addresses: Vec<BindAddress>,
    handle: ServerHandle,
    shutdown_future: Option<Pin<Box<dyn Future<Output = ()>>>>,
}

impl RestApi {
    pub(super) fn new(
        bind_url: String,
        bind_acceptor_builder: Option<SslAcceptorBuilder>,
        resource_providers: Vec<Box<dyn ResourceProvider>>,
        identity_providers: Vec<Box<dyn IdentityProvider>>,
        #[cfg(feature = "store-factory")] store_factory: Option<Box<dyn StoreFactory + Send>>,
        #[cfg(feature = "authorization")] authorization_handlers: Vec<
            Box<dyn AuthorizationHandler>,
        >,
    ) -> Result<Self, RestApiServerError> {
        let providers: Arc<Mutex<Vec<_>>> = Arc::new(Mutex::new(resource_providers));
        #[cfg(feature = "authorization")]
        let permission_map = Arc::new(RwLock::new(PermissionMap::new()));
        let sys = System::new();
        #[cfg(feature = "store-factory")]
        let store_factory = store_factory.map(|factory| Arc::new(Mutex::new(factory)));

        let mut http_server = HttpServer::new(move || {
            let auth_transform = super::auth::AuthTransform::new(
                identity_providers.clone(),
                #[cfg(feature = "authorization")]
                authorization_handlers.clone(),
                #[cfg(feature = "authorization")]
                permission_map.clone(),
            );
            let mut app = App::new();
            #[cfg(feature = "store-factory")]
            {
                if let Some(factory) = &store_factory {
                    app = app.app_data(factory.clone());
                }
            }

            let mut app = app.wrap(middleware::Logger::default()).wrap(auth_transform);
            let pros = providers.lock().unwrap();

            for provider in pros.iter() {
                for resource in provider.resources() {
                    app = app.service(resource)
                }
            }
            app
        });

        http_server = match if let Some(acceptor_builder) = bind_acceptor_builder {
            #[cfg(feature = "https-bind")]
            {
                http_server.bind_openssl(&bind_url, acceptor_builder)
            }
            #[cfg(not(feature = "https-bind"))]
            {
                http_server.bind(&bind_url)
            }
        } else {
            http_server.bind(&bind_url)
        } {
            Ok(http_server) => http_server,
            Err(err1) => {
                let error_msg = format!("Bind to \"{}\" failed", bind_url);
                return Err(RestApiServerError::StartUpError(format!(
                    "{}: {}",
                    error_msg, err1
                )));
            }
        };

        let bind_addresses = http_server
            .addrs_with_scheme()
            .iter()
            .map(|(addr, scheme)| BindAddress {
                addr: *addr,
                scheme: scheme.to_string(),
            })
            .collect();

        let server = http_server.disable_signals().system_exit().run();
        let handle = server.handle();

        // Send the server and bind addresses to the parent thread
        /*
        if let Err(err) = sender.send(FromThreadMessage::Running(server, bind_addresses)) {
            error!("Unable to send running message to parent thread: {}", err);
            return;
        }*/

        match sys.block_on(server) {
            Ok(()) => info!("Rest API terminating"),
            Err(err) => error!("REST API unexpectedly exiting: {}", err),
        };
        Ok(RestApi {
            bind_addresses,
            handle,
            shutdown_future: None,
        })
    }

    /// Returns the list of addresses to which this REST API is bound.
    pub fn bind_addresses(&self) -> &Vec<BindAddress> {
        &self.bind_addresses
    }
}

/// Contains information about the ports to which the REST API is bound.
#[derive(Debug)]
pub struct BindAddress {
    /// The SocketAddr which defines the bound port.
    pub addr: SocketAddr,

    /// The scheme (such as http) that is running on this port.
    pub scheme: String,
}

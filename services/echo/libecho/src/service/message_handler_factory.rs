// Copyright 2018-2022 Cargill Incorporated
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use splinter::service::{MessageHandlerFactory, Routable, ServiceType};

use crate::store::PooledEchoStoreFactory;

use super::message_handler::EchoMessageHandler;

const ECHO_SERVICE_TYPES: &[ServiceType<'static>] = &[ServiceType::new_static("echo")];

#[derive(Clone)]
pub struct EchoMessageHandlerFactory {
    store_factory: Box<dyn PooledEchoStoreFactory>,
}

impl EchoMessageHandlerFactory {
    pub fn new(store_factory: Box<dyn PooledEchoStoreFactory>) -> Self {
        Self { store_factory }
    }
}

impl MessageHandlerFactory for EchoMessageHandlerFactory {
    type MessageHandler = EchoMessageHandler;

    fn new_handler(&self) -> Self::MessageHandler {
        EchoMessageHandler::new(self.store_factory.new_store())
    }

    fn clone_boxed(&self) -> Box<dyn MessageHandlerFactory<MessageHandler = Self::MessageHandler>> {
        Box::new(self.clone())
    }
}

impl Routable for EchoMessageHandlerFactory {
    fn service_types(&self) -> &[ServiceType] {
        ECHO_SERVICE_TYPES
    }
}

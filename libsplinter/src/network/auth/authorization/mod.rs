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
#[cfg(feature = "challenge-authorization")]
pub mod challenge;
#[cfg(feature = "trust-authorization")]
pub mod trust;
pub mod trust_v0;

use crate::error::InvalidStateError;
use crate::network::dispatch::{ConnectionId, Handler, RawBytes};
use crate::protos::authorization::AuthorizationMessageType;

pub type AuthDispatchHandler = Box<
    dyn Handler<Message = RawBytes, MessageType = AuthorizationMessageType, Source = ConnectionId>,
>;

/// Trait for defining an authorization type
pub trait Authorization {
    /// get message handlers for authorization type
    fn get_handlers(&mut self) -> Result<Vec<AuthDispatchHandler>, InvalidStateError>;
}

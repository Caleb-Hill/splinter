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

//! Splinter administrative components.

#[cfg(feature = "admin-service-client")]
pub mod client;
pub mod error;
pub mod lifecycle;
pub mod messages;
#[cfg(any(feature = "rest-api-actix-web-1", feature = "rest-api-actix-web-3"))]
pub mod rest_api;
pub mod service;
pub mod store;
mod token;

pub const CIRCUIT_PROTOCOL_VERSION: i32 = 2;

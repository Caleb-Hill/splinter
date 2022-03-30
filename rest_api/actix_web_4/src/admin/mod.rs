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

#[cfg(feature = "admin")]
mod get_admin_circuits;
#[cfg(feature = "admin")]
mod post_admin_submit;

use actix_web::{web, Resource};

use crate::resource_provider::ResourceProvider;

pub struct AdminResourceProvider {}

impl ResourceProvider for AdminResourceProvider {
    fn resources(&self) -> Vec<Resource> {
        let mut vec = Vec::new();

        #[cfg(feature = "admin")]
        vec.push(web::resource("/").route(web::get().to(get_admin_circuits::get_admin_circuits)));

        vec
    }
}

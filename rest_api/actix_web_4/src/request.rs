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

// There are at least three versions of Request in this crate so the rename is
// worth it.

use std::convert::From;

use actix_web::web::Query;
use actix_web::HttpRequest;
use splinter_rest_api_common::request::Request as CommonRequest;

struct Request {
    pub headers: HashMap<String, String>,
    pub query: HashMap<String, String>,
    pub uri: String,
}

impl From<HttpRequest> for Request {
    fn from(source: HttpRequest) -> Self {
        Self {
            headers: source.headers(),
            query: Query::<HashMap<_, _>>::from_query(source.query_string()).unwrap(),
            uri: source.uri().path().to_string(),
        }
    }
}

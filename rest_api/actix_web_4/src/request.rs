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

use std::collections::HashMap;
use std::convert::From;

use actix_web::web::Query;
use actix_web::{HttpMessage, HttpRequest};
use splinter_rest_api_common::request::Request;

use crate::into_protobuf::{into_protobuf, payload_bytes};

pub struct RequestWrapper<'a> {
    inner: &'a HttpRequest,
}

impl Request for RequestWrapper<'_> {
    fn uri(&self) -> &str {
        self.inner.uri().path()
    }

    fn get_header_value(&self, key: &str) -> Option<Vec<u8>> {
        self.inner
            .head()
            .headers()
            .get(key)
            .map(|a| a.as_bytes().into())
    }

    fn get_header_values(&self, key: &str) -> Box<dyn Iterator<Item = Vec<u8>>> {
        let headers: Vec<Vec<u8>> = self
            .inner
            .head()
            .headers()
            .get_all(key)
            .map(|a| a.as_bytes().into())
            .collect::<Vec<_>>();
        Box::new(headers.into_iter())
    }

    fn get_query_value(&self, key: &str) -> Option<String> {
        match Query::<HashMap<String, String>>::from_query(self.inner.query_string()) {
            Ok(map) => map.get(key).map(|a| a.to_owned()),
            Err(_) => None,
        }
    }

    fn get_body_bytes(&self) -> Vec<u8> {
        let future = payload_bytes(self.inner.take_payload().into());
        // This is the bit I am having trouble with
    }
}

impl<'a> From<&'a HttpRequest> for RequestWrapper<'a> {
    fn from(inner: &'a HttpRequest) -> Self {
        Self { inner }
    }
}

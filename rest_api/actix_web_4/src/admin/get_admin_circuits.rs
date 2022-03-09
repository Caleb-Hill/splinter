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
use std::convert::TryFrom;
use std::sync::{Arc,Mutex};

use splinter::store::StoreFactory;
use splinter::admin::store::AdminServiceStore;
use splinter_rest_api_common::resources::admin::get_admin_circuits::{v1, v2};
use splinter_rest_api_common::paging::{DEFAULT_LIMIT, DEFAULT_OFFSET};

use actix_web::body::BoxBody;
use actix_web::web::Query;
use actix_web::{HttpRequest, HttpResponse, Responder};

use crate::error::RestError;

use crate::protocol_version::{
    ProtocolVersion, MAX_PROTOCOL_VERSION, MIN_PROTOCOL_VERSION,
};

pub async fn get_admin_circuits(request: HttpRequest) -> Result<HttpResponse<BoxBody>, RestError> {
    match ProtocolVersion::try_from(&request) {
        Ok(system_version) => match system_version.into() {
            MIN_PROTOCOL_VERSION..=1 =>{
                let response: V1Response = v1::get_admin_circuits(V1Arguments::try_from(&request)?.into())?.into();
                Ok(response.respond_to(&request))
            }
            2..=MAX_PROTOCOL_VERSION =>{
                let response: V2Response = v2::get_admin_circuits(V2Arguments::try_from(&request)?.into())?.into();
                Ok(response.respond_to(&request))
            }
            // this should be unreachable as ProtocolVersion does the check
            _ => Err(RestError::BadRequest(
                "Protocol version does not have a mapped resource version".to_string()
            )),
        },
        Err(_) => Ok(HttpResponse::Ok().body("Could not get resource")),
    }
}

struct V1Arguments {
    pub store: Box<dyn AdminServiceStore>,
    pub offset: usize,
    pub limit: usize,
    pub link: String,
    pub status: Option<String>,
    pub member: Option<String>,
}

impl Into<v1::Arguments> for V1Arguments {
    fn into(self) -> v1::Arguments {
        v1::Arguments {
            store: self.store ,
            offset: self.offset ,
            limit: self.limit ,
            link: self.link ,
            status: self.status ,
            member: self.member ,
        }
    }
}


impl TryFrom<&HttpRequest> for V1Arguments {
    type Error = RestError;
    fn try_from(value: &HttpRequest) -> Result<Self, Self::Error> {
        let store = value
            .app_data::<Arc<Mutex<Box<dyn StoreFactory + Send>>>>()
            .ok_or_else(|| {
                RestError::InternalError("Could not get StoreFactory from application".to_string(), None)
            })?
            .lock().unwrap()
            .get_admin_service_store();
        let query = Query::<HashMap<String, String>>::from_query(value.query_string()).unwrap();
        let limit = query
            .get("limit")
            .map(|v| v.parse::<usize>())
            .transpose()
            .map_err(|e| RestError::BadRequest(format!("Could not parse limit query: {}", e)))?
            .unwrap_or(DEFAULT_LIMIT);
        let offset = query
            .get("offset")
            .map(|v| v.parse::<usize>())
            .transpose()
            .map_err(|e| RestError::BadRequest(format!("Could not parse offset query: {}", e)))?
            .unwrap_or(DEFAULT_OFFSET);
        let status = query.get("status").map(ToString::to_string);
        let member = query.get("member").map(ToString::to_string);
        let link = value.uri().path().to_string();
        Ok(Self {
            store,
            limit,
            offset,
            member,
            link,
            status,
        })
    }
}

struct V1Response {
    inner: v1::Response,
}

impl From<v1::Response> for V1Response {
    fn from(inner: v1::Response) -> Self {
        Self {
            inner
        }
    }
}

impl Responder for V1Response {
    type Body = BoxBody;
    fn respond_to(self, _req: &HttpRequest) -> HttpResponse<Self::Body> {
        HttpResponse::Ok().json(self.inner)
    }
}

pub struct V2Arguments {
    pub store: Box<dyn AdminServiceStore>,
    pub offset: usize,
    pub limit: usize,
    pub link: String,
    pub status: Option<String>,
    pub member: Option<String>,
}

impl Into<v2::Arguments> for V2Arguments {
    fn into(self) -> v2::Arguments {
        v2::Arguments {
            store: self.store ,
            offset: self.offset ,
            limit: self.limit ,
            link: self.link ,
            status: self.status ,
            member: self.member ,
        }
    }
}

impl TryFrom<&HttpRequest> for V2Arguments {
    type Error = RestError;
    fn try_from(value: &HttpRequest) -> Result<Self, Self::Error> {
        let store = value
            .app_data::<Arc<Mutex<Box<dyn StoreFactory + Send>>>>()
            .ok_or_else(|| {
                RestError::InternalError("Could not get StoreFactory from application".to_string(), None)
            })?
            .lock().unwrap()
            .get_admin_service_store();
        let query = Query::<HashMap<String, String>>::from_query(value.query_string()).unwrap();
        let limit = query
            .get("limit")
            .map(|v| v.parse::<usize>())
            .transpose()
            .map_err(|e| RestError::BadRequest(format!("Could not parse limit query: {}", e)))?
            .unwrap_or(DEFAULT_LIMIT);
        let offset = query
            .get("offset")
            .map(|v| v.parse::<usize>())
            .transpose()
            .map_err(|e| RestError::BadRequest(format!("Could not parse offset query: {}", e)))?
            .unwrap_or(DEFAULT_OFFSET);
        let status = query.get("status").map(ToString::to_string);
        let member = query.get("member").map(ToString::to_string);
        let link = value.uri().path().to_string();
        Ok(Self {
            store,
            limit,
            offset,
            member,
            link,
            status,
        })
    }
}

struct V2Response {
    inner: v2::Response,
}

impl From<v2::Response> for V2Response {
    fn from(inner: v2::Response) -> Self {
        Self {
            inner
        }
    }
}

impl Responder for V2Response {
    type Body = BoxBody;
    fn respond_to(self, _req: &HttpRequest) -> HttpResponse<Self::Body> {
        HttpResponse::Ok().json(self.inner)
    }
}

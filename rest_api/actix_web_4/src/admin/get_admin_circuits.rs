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


use std::convert::TryFrom;
use std::sync::{Arc,Mutex};

use splinter::store::StoreFactory;
use splinter_rest_api_common::resources::admin::get_admin_circuits::{v1, v2};

use actix_web::body::BoxBody;
use actix_web::{HttpRequest, HttpResponse, Responder};

use crate::error::RestError;
use crate::request::RequestWrapper;
use crate::response::JsonResponse;

use crate::protocol_version::{
    ProtocolVersion, MAX_PROTOCOL_VERSION, MIN_PROTOCOL_VERSION,
};

pub async fn get_admin_circuits(request: HttpRequest) -> Result<HttpResponse<BoxBody>, RestError> {
    let store = request.app_data::<Arc<Mutex<Box<dyn StoreFactory + Send >>>>()
        .ok_or_else(|| RestError::InternalError("Could not get store factory from app".into(),None))?
        .lock().unwrap().get_admin_service_store();
    match ProtocolVersion::try_from(&request) {
        Ok(system_version) => match system_version.into() {
            MIN_PROTOCOL_VERSION..=1 =>{
                let args: v1::Arguments = v1::Arguments::new(RequestWrapper::from(&request))?;
                let response = JsonResponse::new(v1::get_admin_circuits(args,store)?);
                Ok(response.respond_to(&request))
            }
            2..=MAX_PROTOCOL_VERSION =>{
                let args: v2::Arguments = v2::Arguments::new(RequestWrapper::from(&request))?;
                let response = JsonResponse::new(v2::get_admin_circuits(args,store)?);
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


impl Responder for v1::Response {
    type Body = BoxBody;
    fn respond_to(self, _req: &HttpRequest) -> HttpResponse<Self::Body> {
        HttpResponse::Ok().json(self)
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
        HttpResponse::Ok().json(self)
    }
}

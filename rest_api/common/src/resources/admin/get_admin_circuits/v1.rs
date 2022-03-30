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

use std::collections::BTreeMap;
use std::convert::From;

use serde::Serialize;
use splinter::admin::store::{
    AdminServiceStore, Circuit, CircuitPredicate, CircuitStatus, Service,
};

use crate::error::ResponseError;
use crate::paging::v1::{Paging, DEFAULT_LIMIT, DEFAULT_OFFSET};
use crate::request::Request;

pub struct Arguments {
    pub offset: usize,
    pub limit: usize,
    pub link: String,
    pub status: Option<String>,
    pub member: Option<String>,
}

impl Arguments {
    pub fn new<T: Request>(source: T) -> Result<Self, ResponseError> {
        let limit = source
            .get_query_value("limit")
            .map(|v| v.parse::<usize>())
            .transpose()
            .map_err(|e| {
                ResponseError::BadRequest(format!("Could not parse limit source.query: {}", e))
            })?
            .unwrap_or(DEFAULT_LIMIT);
        let offset = source
            .get_query_value("offset")
            .map(|v| v.parse::<usize>())
            .transpose()
            .map_err(|e| {
                ResponseError::BadRequest(format!("Could not parse offset source.query: {}", e))
            })?
            .unwrap_or(DEFAULT_OFFSET);
        let status = source.get_query_value("status");
        let member = source.get_query_value("member");
        let link = source.uri().to_string();
        Ok(Self {
            limit,
            offset,
            status,
            member,
            link,
        })
    }
}

pub fn get_admin_circuits(
    args: Arguments,
    store: Box<dyn AdminServiceStore>,
) -> Result<Response, ResponseError> {
    let mut filters = {
        if let Some(member) = args.member {
            vec![CircuitPredicate::MembersInclude(vec![format!(
                "filter={}",
                member
            )])]
        } else {
            vec![]
        }
    };
    if let Some(status) = args.status {
        filters.push(CircuitPredicate::CircuitStatus(match &*status {
            "disbanded" => CircuitStatus::Disbanded,
            "abandoned" => CircuitStatus::Abandoned,
            _ => CircuitStatus::Active,
        }));
    }
    let circuits = store
        .list_circuits(&filters)
        .map_err(|e| ResponseError::internal_error("Error getting circuits", Some(Box::new(e))))?;
    let offset_value = args.offset;
    let total = circuits.len();
    let limit_value = args.limit;

    let data = circuits
        .skip(offset_value)
        .take(limit_value)
        .map(CircuitResponse::from)
        .collect::<Vec<_>>();

    let paging = Paging::builder()
        .with_link(args.link)
        .with_query_count(total)
        .with_offset(args.offset)
        .with_limit(args.limit)
        .build();
    Ok(Response { data, paging })
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct Response {
    pub data: Vec<CircuitResponse>,
    pub paging: Paging,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct CircuitResponse {
    pub id: Box<str>,
    pub members: Vec<String>,
    pub roster: Vec<ServiceResponse>,
    pub management_type: Box<str>,
}

impl From<Circuit> for CircuitResponse {
    fn from(circuit: Circuit) -> Self {
        Self {
            id: circuit.circuit_id().to_string().into(),
            members: circuit
                .members()
                .iter()
                .map(|node| node.node_id().to_string())
                .collect(),
            roster: circuit.roster().iter().map(ServiceResponse::from).collect(),
            management_type: circuit.circuit_management_type().to_string().into(),
        }
    }
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct ServiceResponse {
    pub service_id: Box<str>,
    pub service_type: Box<str>,
    pub allowed_nodes: Vec<String>,
    pub arguments: BTreeMap<String, String>,
}

impl From<&Service> for ServiceResponse {
    fn from(service_def: &Service) -> Self {
        Self {
            service_id: service_def.service_id().to_string().into(),
            service_type: service_def.service_type().to_string().into(),
            allowed_nodes: vec![service_def.node_id().to_string()],
            arguments: service_def
                .arguments()
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect::<BTreeMap<String, String>>(),
        }
    }
}

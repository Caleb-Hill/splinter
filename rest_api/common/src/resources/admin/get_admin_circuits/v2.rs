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
use std::convert::TryFrom;

use serde::Serialize;
use splinter::admin::store::{
    AdminServiceStore, Circuit, CircuitNode, CircuitPredicate, CircuitStatus, Service,
};

use crate::error::ResponseError;
use crate::hex::to_hex;
use crate::paging::{get_response_paging_info, Paging, DEFAULT_LIMIT, DEFAULT_OFFSET};
use crate::request::Request;

pub struct Arguments {
    pub offset: usize,
    pub limit: usize,
    pub link: String,
    pub status: Option<String>,
    pub member: Option<String>,
}

impl<T: Request> TryFrom<T> for Arguments {
    type Error = ResponseError;
    fn try_from(source: T) -> Result<Self, Self::Error> {
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
        let status = source.get_query_value("status").map(ToString::to_string);
        let member = source.get_query_value("member").map(ToString::to_string);
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

    let paging = get_response_paging_info(Some(args.limit), Some(args.offset), &args.link, total);
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
    pub members: Vec<CircuitNodeResponse>,
    pub roster: Vec<ServiceResponse>,
    pub management_type: Box<str>,
    pub display_name: Option<String>,
    pub circuit_version: i32,
    pub circuit_status: CircuitStatus,
}

impl From<Circuit> for CircuitResponse {
    fn from(circuit: Circuit) -> Self {
        Self {
            id: circuit.circuit_id().to_string().into(),
            members: circuit
                .members()
                .iter()
                .map(CircuitNodeResponse::from)
                .collect(),
            roster: circuit.roster().iter().map(ServiceResponse::from).collect(),
            management_type: circuit.circuit_management_type().to_string().into(),
            display_name: circuit.display_name().clone(),
            circuit_version: circuit.circuit_version(),
            circuit_status: circuit.circuit_status().clone(),
        }
    }
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct ServiceResponse {
    pub service_id: Box<str>,
    pub service_type: Box<str>,
    pub node_id: Box<str>,
    pub arguments: BTreeMap<String, String>,
}

impl From<&Service> for ServiceResponse {
    fn from(service_def: &Service) -> Self {
        Self {
            service_id: service_def.service_id().to_string().into(),
            service_type: service_def.service_type().to_string().into(),
            node_id: service_def.node_id().to_string().into(),
            arguments: service_def
                .arguments()
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect::<BTreeMap<String, String>>(),
        }
    }
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct CircuitNodeResponse {
    pub node_id: Box<str>,
    pub endpoints: Vec<String>,
    pub public_key: Option<String>,
}

impl From<&CircuitNode> for CircuitNodeResponse {
    fn from(node_def: &CircuitNode) -> Self {
        Self {
            node_id: node_def.node_id().to_string().into(),
            endpoints: node_def.endpoints().to_vec(),
            public_key: node_def
                .public_key()
                .as_ref()
                .map(|public_key| to_hex(public_key.as_slice())),
        }
    }
}

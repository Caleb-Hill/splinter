// Copyright 2018-2021 Cargill Incorporated
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

use crate::admin::store::{Circuit, CircuitStatus, Service};

#[derive(Debug, Serialize, Clone, PartialEq)]
pub(crate) struct CircuitResponse<'a> {
    pub id: &'a str,
    pub members: Vec<String>,
    pub roster: Vec<ServiceResponse<'a>>,
    pub management_type: &'a str,
    pub display_name: &'a Option<String>,
    pub circuit_version: i32,
    pub circuit_status: &'a CircuitStatus,
}

impl<'a> From<&'a Circuit> for CircuitResponse<'a> {
    fn from(circuit: &'a Circuit) -> Self {
        Self {
            id: circuit.circuit_id(),
            members: circuit
                .members()
                .iter()
                .map(|node| node.node_id().to_string())
                .collect(),
            roster: circuit.roster().iter().map(ServiceResponse::from).collect(),
            management_type: circuit.circuit_management_type(),
            display_name: circuit.display_name(),
            circuit_version: circuit.circuit_version(),
            circuit_status: circuit.circuit_status(),
        }
    }
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub(crate) struct ServiceResponse<'a> {
    pub service_id: &'a str,
    pub service_type: &'a str,
    pub node_id: &'a str,
    pub arguments: BTreeMap<String, String>,
}

impl<'a> From<&'a Service> for ServiceResponse<'a> {
    fn from(service_def: &'a Service) -> Self {
        Self {
            service_id: service_def.service_id(),
            service_type: service_def.service_type(),
            node_id: service_def.node_id(),
            arguments: service_def
                .arguments()
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect::<BTreeMap<String, String>>(),
        }
    }
}

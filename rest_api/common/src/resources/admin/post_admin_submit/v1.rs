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

use crate::request::Request;
use splinter::admin::service::{AdminCommands, AdminServiceError};
use splinter::protos::admin::CircuitManagementPayload;
use splinter::service::ServiceError;

use crate::error::ResponseError;

pub struct Arguments {
    message: CircuitManagementPayload,
}

impl Arguments {
    //fn new<R: Request>(request: R) -> Result<Self, ResponseError> {}
}

pub fn post_admin_submit<A: AdminCommands + Clone + 'static>(
    args: Arguments,
    admin_commands: A,
) -> Result<(), ResponseError> {
    match admin_commands.submit_circuit_change(args.message) {
        Ok(()) => Ok(()),
        Err(AdminServiceError::ServiceError(ServiceError::UnableToHandleMessage(err))) => Err(
            ResponseError::bad_request(format!("Unable to handle message: {}", err)),
        ),
        Err(AdminServiceError::ServiceError(ServiceError::InvalidMessageFormat(err))) => Err(
            ResponseError::bad_request(format!("Failed to parse payload: {}", err)),
        ),
        Err(err) => Err(ResponseError::internal_error("", Some(Box::new(err)))),
    }
}

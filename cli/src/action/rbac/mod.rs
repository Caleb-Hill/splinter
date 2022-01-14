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

//! Actions for handling role-based access control subcommands.

mod assignments;
mod roles;

use clap::ArgMatches;

use crate::action::api::{SplinterRestClient, SplinterRestClientBuilder};
use crate::action::{DEFAULT_SPLINTER_REST_API_URL, SPLINTER_REST_API_URL_ENV};
use crate::error::CliError;
use crate::signing::{create_cylinder_jwt_auth, load_signer};

pub use assignments::{
    CreateAssignmentAction, DeleteAssignmentAction, ListAssignmentsAction, ShowAssignmentAction,
    UpdateAssignmentAction,
};
pub use roles::{
    CreateRoleAction, DeleteRoleAction, ListRolesAction, ShowRoleAction, UpdateRoleAction,
};

/// Constructs a new Splinter REST client from the CLI arguments.
fn new_client(arg_matches: &Option<&ArgMatches<'_>>) -> Result<SplinterRestClient, CliError> {
    let url = arg_matches
        .and_then(|args| args.value_of("url"))
        .map(ToOwned::to_owned)
        .or_else(|| std::env::var(SPLINTER_REST_API_URL_ENV).ok())
        .unwrap_or_else(|| DEFAULT_SPLINTER_REST_API_URL.to_string());

    let signer = load_signer(arg_matches.and_then(|args| args.value_of("private_key_file")))?;

    SplinterRestClientBuilder::new()
        .with_url(url)
        .with_auth(create_cylinder_jwt_auth(signer)?)
        .build()
}

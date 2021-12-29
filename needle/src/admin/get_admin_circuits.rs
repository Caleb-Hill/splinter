use std::error::Error;

use super::resources;
use crate::inputs::{
    header::{BaseLink, ProtocolVersion},
    query::{FilterQuery, Limit, Offset, StatusQuery},
    stores::Store,
};

use actix_utils::future::{err, ok, Ready};
use actix_web::{FromRequest, HttpRequest, HttpResponse, Responder};

use splinter::admin::store::{AdminServiceStore, Circuit, CircuitPredicate, CircuitStatus};
use splinter::rest_api::paging::get_response_paging_info;
use splinter::rest_api::paging::Paging;
use splinter::rest_api::ErrorResponse;

pub async fn get_admin_circuits(
    store: Store<Box<dyn AdminServiceStore>>,
    offset: Offset,
    limit: Limit,
    link: BaseLink,
    status: StatusQuery,
    member: FilterQuery,
) -> Result<PaginatedCircuitList, actix_web::error::BlockingError<TempError>> {
    actix_web::web::block(move || {
        let mut filters = {
            if let Some(member) = &*member {
                vec![CircuitPredicate::MembersInclude(vec![format!(
                    "filter={}",
                    member
                )])]
            } else {
                vec![]
            }
        };
        if let Some(status) = &*status {
            filters.push(CircuitPredicate::CircuitStatus(CircuitStatus::from(
                format!("status={}", status),
            )));
        }

        let circuits = store
            .into_inner()
            .list_circuits(&filters)
            .map_err(|err| TempError(err.to_string()))?;

        let offset_value = *offset;
        let total = circuits.len();
        let limit_value = *limit;

        let circuits = circuits
            .skip(offset_value)
            .take(limit_value)
            .collect::<Vec<_>>();
        let paging = get_response_paging_info(Some(*limit), Some(*offset), &*link, total as usize);
        Ok(PaginatedCircuitList { circuits, paging })
    })
    .await
}

#[derive(Debug)]
pub struct TempError(String);

impl Error for TempError {}

impl std::fmt::Display for TempError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub struct PaginatedCircuitList {
    pub circuits: Vec<Circuit>,
    pub paging: Paging,
}

impl Responder for PaginatedCircuitList {
    type Future = Ready<Result<HttpResponse, Self::Error>>;
    type Error = ();
    fn respond_to(self, req: &HttpRequest) -> Self::Future {
        if let Ok(protocol_version) = ProtocolVersion::extract(req).into_inner() {
            match protocol_version {
                ProtocolVersion::One => ok(HttpResponse::Ok().json(
                    resources::v1::circuits::ListCircuitsResponse {
                        data: self
                            .circuits
                            .iter()
                            .map(resources::v1::circuits::CircuitResponse::from)
                            .collect(),
                        paging: self.paging,
                    },
                )),

                // Handles 2
                ProtocolVersion::Two => ok(HttpResponse::Ok().json(
                    resources::v2::circuits::ListCircuitsResponse {
                        data: self
                            .circuits
                            .iter()
                            .map(resources::v2::circuits::CircuitResponse::from)
                            .collect(),
                        paging: self.paging,
                    },
                )),
                _ => ok(
                    HttpResponse::BadRequest().json(ErrorResponse::bad_request(&format!(
                        "Unsupported SplinterProtocolVersion: {}",
                        protocol_version
                    ))),
                ),
            }
        } else {
            err(())
        }
    }
}

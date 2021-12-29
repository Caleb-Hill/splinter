use std::collections::HashMap;
use std::ops::Deref;

use actix_utils::future::{err, ok, Ready};
use actix_web::{dev::Payload, web::Query, FromRequest, HttpRequest};

use splinter::rest_api::paging::DEFAULT_OFFSET;

pub struct Offset {
    value: usize,
}

impl Deref for Offset {
    type Target = usize;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl FromRequest for Offset {
    type Error = ();
    type Future = Ready<Result<Self, Self::Error>>;
    type Config = ();
    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let query: Query<HashMap<String, String>> =
            if let Ok(q) = Query::from_query(req.query_string()) {
                q
            } else {
                return err(());
            };
        match query.get("offset") {
            Some(value) => match value.parse::<usize>() {
                Ok(value) => ok(Self { value }),
                Err(_) => return err(()),
            },
            None => ok(Self {
                value: DEFAULT_OFFSET,
            }),
        }
    }
}

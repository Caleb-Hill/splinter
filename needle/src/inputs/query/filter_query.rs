use std::collections::HashMap;
use std::ops::Deref;

use actix_utils::future::{err, ok, Ready};
use actix_web::{dev::Payload, web::Query, FromRequest, HttpRequest};

pub struct FilterQuery {
    value: Option<String>,
}

impl Deref for FilterQuery {
    type Target = Option<String>;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}
impl FromRequest for FilterQuery {
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
        ok(Self {
            value: query.get("filter").map(|v| v.to_string()),
        })
    }
}

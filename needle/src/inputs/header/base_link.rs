use std::ops::Deref;

use actix_utils::future::{ok, Ready};
use actix_web::{dev::Payload, FromRequest, HttpRequest};

pub struct BaseLink {
    value: String,
}

impl Deref for BaseLink {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        self.value.as_ref()
    }
}

impl FromRequest for BaseLink {
    type Error = ();
    type Future = Ready<Result<Self, Self::Error>>;
    type Config = ();
    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let value = req.uri().path().to_string();
        ok(Self { value })
    }
}

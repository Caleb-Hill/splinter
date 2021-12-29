use std::fmt::Display;

use actix_utils::future::{err, ok, Ready};
use actix_web::{dev::Payload, FromRequest, HttpRequest};

use crate::inputs::error::InputError;

#[non_exhaustive]
pub enum ProtocolVersion {
    One,
    Two,
    Three,
}

impl Display for ProtocolVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val = match self {
            ProtocolVersion::One => "1",
            ProtocolVersion::Two => "2",
            ProtocolVersion::Three => "3",
        };
        write!(f, "{}", val)
    }
}

impl FromRequest for ProtocolVersion {
    type Error = InputError;
    type Future = Ready<Result<Self, Self::Error>>;
    type Config = ();
    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        match req.headers().get("SplinterProtocolVersion") {
            Some(header_value) => match header_value.to_str() {
                Ok(protocol_version) => match protocol_version {
                    "1" => ok(ProtocolVersion::One),
                    "2" => ok(ProtocolVersion::Two),
                    "3" => ok(ProtocolVersion::Three),
                    _ => err(InputError::InvalidValue(
                        "protocol_version is unsupported".to_string(),
                    )),
                },
                Err(_) => err(InputError::InvalidValue(
                    "Could not convert header to str".to_string(),
                )),
            },
            None => ok(ProtocolVersion::Three),
        }
    }
}

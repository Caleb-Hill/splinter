use std::error::Error;
use std::fmt::Display;

use actix_web::ResponseError;

#[derive(Debug)]
pub enum InputError {
    InvalidValue(String),
}

impl Error for InputError {}

impl Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Input Error")
    }
}

impl ResponseError for InputError {}

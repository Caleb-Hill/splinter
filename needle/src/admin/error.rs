use std::error::Error;
use std::fmt::Display;

#[derive(Debug)]
pub enum RestAPIError {}

impl Error for RestAPIError {}

impl Display for RestAPIError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            _ => write!(f, "Error"),
        }
    }
}

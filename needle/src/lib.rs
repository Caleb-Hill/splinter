mod admin;
mod hex;
mod inputs;

use actix_web::Resource;

pub trait ResourceProvider: Send {
    /// Returns a list of Actix `Resource`s.
    fn resources(&self) -> Vec<Resource>;
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}

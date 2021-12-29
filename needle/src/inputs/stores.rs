use splinter::admin::store::AdminServiceStore;
use splinter::store::StoreFactory;

use actix_utils::future::{err, ok, Ready};
use actix_web::{dev::Payload, FromRequest, HttpRequest};

// Exists as a wrapper for types not in this crate.
pub struct Store<S>(S);

impl<S> Store<S> {
    pub fn into_inner(self) -> S {
        self.0
    }
}

impl FromRequest for Store<Box<dyn AdminServiceStore>> {
    type Error = ();
    type Future = Ready<Result<Self, Self::Error>>;
    type Config = ();
    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        if let Some(factory) = req.app_data::<Box<dyn StoreFactory>>() {
            ok(Store(factory.get_admin_service_store()))
        } else {
            err(())
        }
    }
}

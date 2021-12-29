mod error;
mod get_admin_circuits;
mod resources;

use actix_web::{web, Resource};

use crate::ResourceProvider;

pub struct AdminResourceProvider {}

impl Default for AdminResourceProvider {
    fn default() -> Self {
        AdminResourceProvider::new()
    }
}

impl AdminResourceProvider {
    pub fn new() -> Self {
        AdminResourceProvider {}
    }
}

impl ResourceProvider for AdminResourceProvider {
    fn resources(&self) -> Vec<Resource> {
        vec![web::resource("/admin/circuits")
            .route(web::get().to(get_admin_circuits::get_admin_circuits))]
    }
}

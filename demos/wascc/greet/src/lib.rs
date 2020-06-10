extern crate wascc_actor as actor;

use std::collections::HashMap;

use actor::prelude::*;

actor_handlers! { codec::http::OP_HANDLE_REQUEST => greet,
codec::core::OP_HEALTH_REQUEST => health }

pub fn greet(r: codec::http::Request) -> HandlerResult<codec::http::Response> {
    println(&format!("Received HTTP request: {:?}", &r));
    Ok(codec::http::Response {
        status_code: 200,
        status: "OK".to_owned(),
        header: HashMap::new(),
        body: b"Hello, world!\n".to_vec(),
    })
}

pub fn health(_h: codec::core::HealthRequest) -> HandlerResult<()> {
    Ok(())
}

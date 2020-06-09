extern crate wascc_actor as actor;

#[macro_use]
extern crate log;
extern crate serde;
extern crate wascc_codec;

use actor::prelude::*;
use serde::Serialize;

actor_handlers! {
    codec::http::OP_HANDLE_REQUEST => uppercase,
    codec::core::OP_HEALTH_REQUEST => health
}

fn uppercase(r: codec::http::Request) -> HandlerResult<codec::http::Response> {
    info!("Query String: {}", r.query_string);
    let upper = UppercaseResponse {
        original: r.query_string.to_string(),
        uppercased: r.query_string.to_ascii_uppercase(),
    };

    Ok(codec::http::Response::json(upper, 200, "OK"))
}

fn health(_req: codec::core::HealthRequest) -> HandlerResult<()> {
    Ok(())
}

#[derive(Serialize)]
struct UppercaseResponse {
    original: String,
    uppercased: String,
}

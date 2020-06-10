extern crate wascc_actor as actor;
extern crate wascc_codec;

use actor::prelude::*;

actor_handlers! {
    codec::http::OP_HANDLE_REQUEST => fetch,
    codec::core::OP_HEALTH_REQUEST => health
}

fn fetch(r: codec::http::Request) -> HandlerResult<codec::http::Response> {
    // k8s volumes are mounted into the waSCC runtime using the same volume mount name
    let store = objectstore::host("storage");
    let mut path = String::from(r.path);

    // strip the leading slash from the path
    path = path.trim_start_matches('/').to_string();

    match r.method.as_str() {
        "GET" => {
            match store.get_blob_info("", path.as_str())? {
                Some(blob) => {
                    if blob.id == "none" {
                        return Ok(codec::http::Response::not_found());
                    }
                    Ok(codec::http::Response::json(blob, 200, "OK"))
                },
                None => Ok(codec::http::Response::not_found()),
            }
        },
        "POST" => {
            let blob = codec::blobstore::Blob {
                id: path,
                container: "".to_owned(),
                byte_size: r.body.len() as u64,
            };
            // TODO: check if this is the start of an upload or another chunk. Right now we accept the request as the only chunk.
            let transfer = store.start_upload(&blob, r.body.len() as u64, r.body.len() as u64)?;
            store.upload_chunk(&transfer, 0, &r.body)?;
            Ok(codec::http::Response::ok())
        }
        "DELETE" => {
            store.remove_object(path.as_str(), "")?;
            Ok(codec::http::Response::ok())
        }
        _ => Ok(codec::http::Response::bad_request()),
    }
}

fn health(_req: codec::core::HealthRequest) -> HandlerResult<()> {
    Ok(())
}

use wasi_experimental_http;
use bytes::Bytes;
use http;

fn main() {
    const POSTMAN_ECHO_PAYLOAD: &[u8] = b"I'm not superstitious, but I am a little stitious.";
    const POSTMAN_ECHO_POST_URL: &str = "https://postman-echo.com/post";

    let request_body = Bytes::from(POSTMAN_ECHO_PAYLOAD);

    let request = http::request::Builder::new()
        .method(http::Method::POST)
        .uri(POSTMAN_ECHO_POST_URL)
        .header("Content-Type", "text/plain")
        .body(Some(request_body))
        .expect("cannot construct request");

    let mut response = wasi_experimental_http::request(request).expect("cannot make request");
    let response_body = response.body_read_all().unwrap();
    let response_text = std::str::from_utf8(&response_body).unwrap().to_string();
    
    println!("{}", response.status_code);
    println!("{}", response_text);
}
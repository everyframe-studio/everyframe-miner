//! A bounded, single-connection ElevenLabs dialogue stream. Neither a lost
//! connection nor a missing final frame causes another synthesis request.
use crate::{
    Error, Result, need,
    network::{Request, public_address, safe_url},
    protocol::decode,
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket, client::IntoClientRequest, protocol::WebSocketConfig};

fn collect<S: Read + Write>(
    socket: &mut WebSocket<S>,
    voices: &Value,
    inputs: &Value,
    limit: usize,
    timeout: Duration,
    submit_deadline: Option<i64>,
) -> Result<Vec<u8>> {
    let started = Instant::now();
    if let Some(deadline) = submit_deadline {
        need(crate::now() < deadline, "submission_deadline_elapsed")?;
    }
    for message in [
        json!({"voices":voices}),
        json!({"inputs":inputs}),
        json!({"close_socket":true}),
    ] {
        socket
            .send(Message::Text(message.to_string().into()))
            .map_err(|_| Error("stream_write_failed"))?;
    }
    let mut audio = Vec::new();
    for _ in 0..100000 {
        need(started.elapsed() < timeout, "request_timeout")?;
        match socket.read().map_err(|_| Error("stream_incomplete"))? {
            Message::Text(text) => {
                let data: Value =
                    serde_json::from_str(&text).map_err(|_| Error("invalid_provider_response"))?;
                need(data["error"].is_null(), "provider_generation_failed")?;
                if let Some(encoded) = data["audio"].as_str() {
                    let bytes = decode(encoded)?;
                    need(
                        audio
                            .len()
                            .checked_add(bytes.len())
                            .is_some_and(|n| n <= limit),
                        "response_too_large",
                    )?;
                    audio.extend(bytes);
                }
                if data["is_final"] == true {
                    need(!audio.is_empty(), "provider_no_media")?;
                    let _ = socket.close(None);
                    return Ok(audio);
                }
            }
            Message::Ping(_) | Message::Pong(_) => (),
            Message::Close(_) => return Err(Error("stream_incomplete")),
            _ => return Err(Error("invalid_provider_response")),
        }
    }
    Err(Error("response_too_large"))
}

pub(crate) fn dialogue(r: Request, voices: &Value, inputs: &Value) -> Result<Vec<u8>> {
    let mut url = safe_url(&r.url, &r.hosts)?;
    need(
        url.host_str() == Some("api.elevenlabs.io")
            && url.path() == "/v1/text-to-dialogue/stream-input"
            && r.method == "GET"
            && r.body.is_none()
            && r.redirect_hosts.is_empty(),
        "unapproved_destination",
    )?;
    let addresses = ("api.elevenlabs.io", 443)
        .to_socket_addrs()
        .map_err(|_| Error("dns_failed"))?
        .collect::<Vec<_>>();
    need(
        !addresses.is_empty() && addresses.iter().all(|a| public_address(a.ip())),
        "private_destination",
    )?;
    let timeout = Duration::from_secs(r.timeout.min(600));
    let stream = TcpStream::connect_timeout(&addresses[0], Duration::from_secs(15))
        .map_err(|_| Error("transport_failed"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|_| Error("transport_failed"))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(30)))
        .map_err(|_| Error("transport_failed"))?;
    url.set_scheme("wss").map_err(|_| Error("invalid_url"))?;
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|_| Error("invalid_url"))?;
    for (k, v) in &r.headers {
        let name = tungstenite::http::header::HeaderName::from_bytes(k.as_bytes())
            .map_err(|_| Error("invalid_header"))?;
        let mut value = tungstenite::http::header::HeaderValue::from_str(v)
            .map_err(|_| Error("invalid_header"))?;
        value.set_sensitive(true);
        request.headers_mut().insert(name, value);
    }
    let config = WebSocketConfig::default()
        .max_message_size(Some(4 * 1024 * 1024))
        .max_frame_size(Some(4 * 1024 * 1024));
    let (mut socket, _) = tungstenite::client_tls_with_config(request, stream, Some(config), None)
        .map_err(|_| Error("stream_connect_failed"))?;
    collect(
        &mut socket,
        voices,
        inputs,
        r.limit,
        timeout,
        r.submit_deadline,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(final_frame: bool, limit: usize) -> Result<Vec<u8>> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let mut messages = vec![];
            for _ in 0..3 {
                messages.push(
                    serde_json::from_str::<Value>(&ws.read().unwrap().into_text().unwrap())
                        .unwrap(),
                );
            }
            assert_eq!(messages[0], json!({"voices":["voice"]}));
            assert_eq!(messages[1]["inputs"][0]["text"], "hello");
            assert_eq!(messages[2], json!({"close_socket":true}));
            let _ = ws.send(Message::Text(json!({"audio":"YWJj"}).to_string().into()));
            if final_frame {
                let _ = ws.send(Message::Text(
                    json!({"audio":"ZGVm","is_final":true}).to_string().into(),
                ));
            } else {
                let _ = ws.close(None);
            }
        });
        let (stream, _) = tungstenite::client(
            format!("ws://{address}"),
            TcpStream::connect(address).unwrap(),
        )
        .unwrap();
        let mut ws = stream;
        let result = collect(
            &mut ws,
            &json!(["voice"]),
            &json!([{"text":"hello","voice_id":"voice"}]),
            limit,
            Duration::from_secs(5),
            None,
        );
        drop(ws);
        server.join().unwrap();
        result
    }
    #[test]
    fn collects_one_stream_and_requires_final_ack() {
        assert_eq!(run(true, 100).unwrap(), b"abcdef");
        assert_eq!(run(false, 100).unwrap_err().0, "stream_incomplete");
    }
    #[test]
    fn bounded_output_and_destination_checks() {
        assert_eq!(run(true, 2).unwrap_err().0, "response_too_large");
        assert!(
            dialogue(
                Request::get(
                    "https://example.com/v1/text-to-dialogue/stream-input",
                    &["example.com"]
                ),
                &json!([]),
                &json!([])
            )
            .is_err()
        );
    }

    #[test]
    fn expired_grant_sends_no_synthesis_messages() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            assert!(ws.read().is_err());
        });
        let (mut ws, _) = tungstenite::client(
            format!("ws://{address}"),
            TcpStream::connect(address).unwrap(),
        )
        .unwrap();
        assert_eq!(
            collect(
                &mut ws,
                &json!(["voice"]),
                &json!([{ "text":"hello","voice_id":"voice" }]),
                100,
                Duration::from_secs(5),
                Some(0)
            )
            .unwrap_err()
            .0,
            "submission_deadline_elapsed"
        );
        drop(ws);
        server.join().unwrap();
    }
}

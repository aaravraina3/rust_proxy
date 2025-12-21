use httparse::Request;
use tracing::info;

pub fn inspect_and_modify(data: &[u8]) -> Option<Vec<u8>> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = Request::new(&mut headers);

    match req.parse(data) {
        Ok(httparse::Status::Complete(_)) => {
            let method = req.method.unwrap_or("UNKNOWN");
            let path = req.path.unwrap_or("UNKNOWN");
            info!("HTTP Request: {} {}", method, path);

            // Simple modification: Inject a header
            // We search for the first \r\n (end of request line) and insert there
            // or just before the double \r\n (end of headers)
            let data_str = String::from_utf8_lossy(data);
            if let Some(pos) = data_str.find("\r\n") {
                let (first_line, rest) = data_str.split_at(pos + 2);
                let mut modified = String::with_capacity(data.len() + 30);
                modified.push_str(first_line);
                modified.push_str("X-Proxy-Handled: true\r\n");
                modified.push_str(rest);
                return Some(modified.into_bytes());
            }
        }
        _ => {}
    }

    None
}


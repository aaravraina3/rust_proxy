use httparse::Request;
use tracing::info;

/// Inspect an HTTP request without copying it.
///
/// `httparse` parses by borrowing slices directly out of `data` (the method,
/// path and headers all point back into the original buffer), so nothing is
/// allocated or copied here. The caller forwards the same buffer onward.
pub fn inspect(data: &[u8]) {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = Request::new(&mut headers);

    if let Ok(httparse::Status::Complete(_)) = req.parse(data) {
        let method = req.method.unwrap_or("UNKNOWN");
        let path = req.path.unwrap_or("UNKNOWN");
        info!("HTTP Request: {} {}", method, path);
    }
}

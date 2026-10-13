use crate::{failed, invalid, request::Config, stream::Stream};
use curl::easy::{Easy2, Handler, List, WriteError};
use phenix_basic_contracts::{error, BasicError, ErrorCode, ModelReply, ModelRequest};
use phenix_sdk::WakePermit;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

struct Transfer {
    stream: Stream,
    progress: Arc<Mutex<String>>,
    cancelled: Arc<AtomicBool>,
    wake: WakePermit,
    status: u32,
    sse: bool,
    failure: Option<BasicError>,
}
impl Handler for Transfer {
    fn header(&mut self, bytes: &[u8]) -> bool {
        let line = String::from_utf8_lossy(bytes);
        if line.starts_with("HTTP/") {
            self.status = line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            self.sse = false;
        } else if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-type") {
                self.sse = value
                    .trim()
                    .split(';')
                    .next()
                    .is_some_and(|v| v.eq_ignore_ascii_case("text/event-stream"));
            }
        }
        if line.trim().is_empty() && self.status >= 300 {
            return false;
        }
        true
    }
    fn write(&mut self, bytes: &[u8]) -> Result<usize, WriteError> {
        if self.cancelled.load(Ordering::Acquire) {
            return Ok(0);
        }
        if !(200..300).contains(&self.status) {
            return Ok(bytes.len());
        }
        if !self.sse {
            self.failure = Some(invalid("HTTP response is not an event stream"));
            return Ok(0);
        }
        match self.stream.feed(bytes) {
            Ok(delta) => {
                if !delta.is_empty() {
                    match self.progress.lock() {
                        Ok(mut progress) => progress.push_str(&delta),
                        Err(_) => {
                            self.failure = Some(failed("HTTP progress queue failed"));
                            return Ok(0);
                        }
                    }
                    self.wake.wake();
                }
                Ok(bytes.len())
            }
            Err(error) => {
                self.failure = Some(error);
                Ok(0)
            }
        }
    }
    fn progress(&mut self, _: f64, _: f64, _: f64, _: f64) -> bool {
        !self.cancelled.load(Ordering::Acquire)
    }
}
pub(crate) fn run(
    config: Config,
    body: Vec<u8>,
    request: ModelRequest,
    progress: Arc<Mutex<String>>,
    cancelled: Arc<AtomicBool>,
    wake: WakePermit,
) -> Result<ModelReply, BasicError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(failed("HTTP model cancelled"));
    }
    let mut handle = Easy2::new(Transfer {
        stream: Stream::default(),
        progress,
        cancelled,
        wake,
        status: 0,
        sse: false,
        failure: None,
    });
    let configured = (|| -> Result<(), curl::Error> {
        handle.url(&config.endpoint)?;
        handle.post(true)?;
        handle.post_fields_copy(&body)?;
        handle.follow_location(false)?;
        handle.signal(false)?;
        handle.ssl_verify_peer(true)?;
        handle.ssl_verify_host(true)?;
        handle.progress(true)?;
        if let Some(milliseconds) = request.limits.http_timeout_ms {
            handle.timeout(Duration::from_millis(milliseconds))?;
        }
        if let Some(ca) = config.ca_file {
            handle.cainfo(ca)?;
        }
        let mut headers = List::new();
        headers.append("Content-Type: application/json")?;
        headers.append("Accept: text/event-stream")?;
        headers.append("Expect:")?;
        if let Some(token) = config.credential {
            headers.append(&format!("Authorization: Bearer {token}"))?;
        }
        handle.http_headers(headers)?;
        Ok(())
    })();
    configured.map_err(|_| failed("HTTP transport configuration failed"))?;
    if handle.get_ref().cancelled.load(Ordering::Acquire) {
        return Err(failed("HTTP model cancelled"));
    }
    let result = handle.perform();
    if let Some(error) = handle.get_mut().failure.take() {
        return Err(error);
    }
    if handle.get_ref().status >= 300 {
        return Err(failed(&format!(
            "HTTP model status {}",
            handle.get_ref().status
        )));
    }
    if let Err(problem) = result {
        return Err(if problem.is_operation_timedout() {
            error(ErrorCode::TurnLimit, "HTTP timeout reached")
        } else {
            failed("HTTP transport failed")
        });
    }
    let status = handle
        .response_code()
        .map_err(|_| failed("missing HTTP status"))?;
    if !(200..300).contains(&status) {
        return Err(failed(&format!("HTTP model status {status}")));
    }
    if !handle.get_ref().sse {
        return Err(invalid("HTTP response is not an event stream"));
    }
    std::mem::take(&mut handle.get_mut().stream).finish(&request)
}

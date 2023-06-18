use async_trait::async_trait;

use super::{HeaderPair, unix::UnixSocketConnector};


pub struct HttpTransport(pub reqwest::Client, pub String);
#[cfg(feature = "unix-socket")]
pub struct UnixSocketTransport(pub hyper::Client<UnixSocketConnector>);

pub enum Transport {
    Http(HttpTransport),
    #[cfg(feature = "unix-socket")]
    UnixSocket(UnixSocketTransport),
}

#[async_trait]
impl TransportImpl for Transport {
    async fn make_request(&self, user_agent: &str, method: http::Method, url: url::Url, json: String, headers: Vec<HeaderPair>) -> super::Result<TransportResult> {
        match self {
            Transport::Http(t) => {
                t.make_request(user_agent, method, url, json, headers).await
            },
            #[cfg(feature = "unix-socket")]
            Transport::UnixSocket(t) => {
                t.make_request(user_agent, method, url, json, headers).await
            },
        }
    }
}

impl From<HttpTransport> for Transport {
    fn from(t: HttpTransport) -> Self {
        Transport::Http(t)
    }
}
#[cfg(feature = "unix-socket")]
impl From<UnixSocketTransport> for Transport {
    fn from(t: UnixSocketTransport) -> Self {
        Transport::UnixSocket(t)
    }
}

pub struct TransportResult {
    pub body: bytes::Bytes,
    pub status_code: http::StatusCode,
    pub request_id: Option<String>,
}

// TODO: Use real async fns as soon as they are stable

#[async_trait]
pub(crate) trait TransportImpl {
    async fn make_request(&self, user_agent: &str, method: http::Method, url: url::Url, json: String, headers: Vec<HeaderPair>) -> super::Result<TransportResult>;
}

#[async_trait]
impl TransportImpl for HttpTransport {
    async fn make_request(&self, user_agent: &str, method: http::Method, url: url::Url, json: String, headers: Vec<HeaderPair>) -> super::Result<TransportResult> {
        let mut builder = self.0
            .request(method, url)
            .body(json)
            .header("User-Agent", user_agent)
            .header("Content-Type", "application/json")
            .header("Authorization", self.1.as_str());

        for HeaderPair(name, value) in headers {
            builder = builder.header(name, value);
        }

        let response = builder.send()
            .await?;

        let status = response.status();
        let fly_request_id = response.headers().get("fly-request-id").and_then(|v| v.to_str().ok().map(str::to_string));
        let resp_bytes = response.bytes().await?;

        Ok(TransportResult {
            body: resp_bytes,
            status_code: status,
            request_id: fly_request_id,
        })
    }
}

#[cfg(feature = "unix-socket")]
#[async_trait]
impl TransportImpl for UnixSocketTransport {
    async fn make_request(&self, user_agent: &str, method: http::Method, url: url::Url, json: String, headers: Vec<HeaderPair>) -> super::Result<TransportResult> {
        let mut builder = hyper::Request::builder()
            .method(method)
            .uri(url.as_str())
            .header("User-Agent", user_agent)
            .header("Content-Type", "application/json");

        for HeaderPair(name, value) in headers {
            builder = builder.header(name, value);
        }

        let req = builder.body(hyper::Body::from(json))?;
        let response = self.0.request(req).await?;

        let status = response.status();
        let fly_request_id = response.headers().get("fly-request-id").and_then(|v| v.to_str().ok().map(str::to_string));
        let resp_bytes = response.into_body();

        Ok(TransportResult {
            body: hyper::body::to_bytes(resp_bytes).await?,
            status_code: status,
            request_id: fly_request_id,
        })
    }
}
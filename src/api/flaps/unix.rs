use std::{path::Path, pin::Pin, task::{Context, Poll}};

use hyper::{Uri, service::Service};
use pin_project::pin_project;
use tokio::io::ReadBuf;

#[pin_project]
pub struct UnixSocketStream(#[pin] tokio::net::UnixStream);

impl UnixSocketStream {
    async fn new<P: AsRef<Path>>(path: P) -> tokio::io::Result<UnixSocketStream> {
        Ok(UnixSocketStream(tokio::net::UnixStream::connect(path).await?))
    }
}

impl tokio::io::AsyncRead for UnixSocketStream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<tokio::io::Result<()>> {
        self.project().0.poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for UnixSocketStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<tokio::io::Result<usize>> {
        self.project().0.poll_write(cx, buf)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        self.project().0.poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        self.project().0.poll_shutdown(cx)
    }
    fn is_write_vectored(&self) -> bool {
        self.0.is_write_vectored()
    }
    fn poll_write_vectored(self: Pin<&mut Self>, cx: &mut Context<'_>, bufs: &[std::io::IoSlice<'_>]) -> Poll<Result<usize, std::io::Error>> {
        self.project().0.poll_write_vectored(cx, bufs)
    }
}

impl hyper::client::connect::Connection for UnixSocketStream {
    fn connected(&self) -> hyper::client::connect::Connected {
        hyper::client::connect::Connected::new()
    }
}

#[derive(Debug, Clone)]
pub struct UnixSocketConnector;

impl Service<Uri> for UnixSocketConnector {
    type Response = UnixSocketStream;
    type Error = tokio::io::Error;
    type Future = Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn call(&mut self, _req: Uri) -> Self::Future {
        Box::pin(async move {
            UnixSocketStream::new("/.fly/api").await
        })
    }
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}
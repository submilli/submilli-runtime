//! Recover an initializing transport when rmcp's handshake future is cancelled.
//! Once initialization succeeds, RunningService owns normal close-and-join.

use rmcp::model::{ClientJsonRpcMessage, ServerJsonRpcMessage};
use rmcp::service::RoleClient;
use rmcp::transport::streamable_http_client::StreamableHttpError;
use rmcp::transport::{StreamableHttpClientTransport, Transport};
use tokio::sync::oneshot;

use super::bounded_client::BoundedClient;

pub(super) type HttpTransport = StreamableHttpClientTransport<BoundedClient>;
pub(super) type PendingTransport = oneshot::Receiver<HttpTransport>;

pub(super) struct DrainingTransport {
    inner: Option<HttpTransport>,
    returned: Option<oneshot::Sender<HttpTransport>>,
}

impl DrainingTransport {
    pub fn new(inner: HttpTransport) -> (Self, PendingTransport) {
        let (returned, receiver) = oneshot::channel();
        (
            Self {
                inner: Some(inner),
                returned: Some(returned),
            },
            receiver,
        )
    }
}

impl Transport<RoleClient> for DrainingTransport {
    type Error = StreamableHttpError<reqwest::Error>;

    fn send(
        &mut self,
        message: ClientJsonRpcMessage,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        match self.inner.as_mut() {
            Some(inner) => futures::future::Either::Left(inner.send(message)),
            None => futures::future::Either::Right(futures::future::ready(Err(
                StreamableHttpError::TransportChannelClosed,
            ))),
        }
    }

    async fn receive(&mut self) -> Option<ServerJsonRpcMessage> {
        match self.inner.as_mut() {
            Some(inner) => inner.receive().await,
            None => None,
        }
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        match self.inner.as_mut() {
            Some(inner) => inner.close().await,
            None => Ok(()),
        }
    }
}

impl Drop for DrainingTransport {
    fn drop(&mut self) {
        if let (Some(returned), Some(inner)) = (self.returned.take(), self.inner.take()) {
            // A live receiver means initialization did not transfer cleanup to
            // RunningService. It will close and join this worker before accounting.
            let _ = returned.send(inner);
        }
    }
}

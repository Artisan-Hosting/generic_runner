use crate::secrets::secret_service::{self, secret_service_client::SecretServiceClient};
use artisan_middleware::dusa_collection_utils::{
    core::{logger::LogLevel, types::rb::RollingBuffer},
    log,
};
use tonic::transport::Channel;

#[derive(Debug, Clone)]
pub struct SecretClient {
    client: SecretServiceClient<Channel>,
    _log: RollingBuffer,
}

impl SecretClient {
    fn log(&mut self, msg: String) {
        log!(LogLevel::Debug, "{}", msg);
        self._log.push(msg);
    }

    pub async fn connect(addr: &String) -> Result<Self, tonic::transport::Error> {
        let mut buffer = RollingBuffer::new(1024);
        let log_msg = format!("Attempting to connect to secret server @ {}", addr);
        log!(LogLevel::Debug, "{}", log_msg);
        buffer.push(log_msg);
        let client = SecretServiceClient::connect(addr.clone()).await?;

        let log_msg = format!("Connected to secret server @ {}", addr);
        log!(LogLevel::Debug, "{}", log_msg);
        buffer.push(log_msg);

        Ok(Self {
            client,
            _log: buffer,
        })
    }

    pub async fn get_all_secrets(
        &mut self,
        req: secret_service::GetAllSecretsRequest,
    ) -> Result<secret_service::GetAllSecretsResponse, tonic::Status> {
        self.log(format!("Requesting all secrets for: {}", req.runner_id));
        Ok(self.client.get_all_secrets(req).await?.into_inner())
    }
}

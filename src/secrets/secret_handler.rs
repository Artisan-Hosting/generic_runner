use crate::secrets::secret_service::{self, secret_service_client::SecretServiceClient};
use artisan_middleware::dusa_collection_utils::{
    core::{logger::LogLevel, types::rb::RollingBuffer},
    log,
};
use tonic::transport::Channel;

#[derive(Clone)]
pub struct SecretClient {
    client: SecretServiceClient<Channel>,
    /// The bearer secret ais_auth issued this runner (`AIS_SERVICE_CREDENTIAL_FILE`
    /// / `AIS_SERVICE_CREDENTIAL`). ais_secretserver only releases the secrets
    /// this credential has been granted -- issue one per app with an exact grant
    /// on `{project_id}/{environment_id}` rather than sharing a broad one, so a
    /// compromised app cannot read its neighbours' secrets. Never logged.
    service_credential: String,
    _log: RollingBuffer,
}

// Hand-written so `{:?}` can never print the bearer credential.
impl std::fmt::Debug for SecretClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretClient")
            .field("service_credential", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl SecretClient {
    fn log(&mut self, msg: String) {
        log!(LogLevel::Debug, "{}", msg);
        self._log.push(msg);
    }

    pub fn service_credential(&self) -> &str {
        &self.service_credential
    }

    /// `addr` chooses the transport: `https://` is mutual TLS against
    /// ais_secretserver (client certificate from `MTLS_*` / `/etc/artisan/tls/
    /// runner.*`); `http://` is plaintext and only works against a dev server.
    pub async fn connect(addr: &String) -> Result<Self, String> {
        let mut buffer = RollingBuffer::new(1024);
        let log_msg = format!("Attempting to connect to secret server @ {}", addr);
        log!(LogLevel::Debug, "{}", log_msg);
        buffer.push(log_msg);
        let service_credential = super::mtls_client::load_service_credential()?;
        let mtls = if super::mtls_client::wants_tls(addr) {
            Some(super::mtls_client::ClientMtls::load("runner")?)
        } else {
            None
        };
        let channel = super::mtls_client::connect_internal(addr, "ais_secretserver", mtls.as_ref()).await?;
        let client = SecretServiceClient::new(channel);

        let log_msg = format!("Connected to secret server @ {}", addr);
        log!(LogLevel::Debug, "{}", log_msg);
        buffer.push(log_msg);

        Ok(Self {
            client,
            service_credential,
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

use crate::secrets::{
    secret_handler::SecretClient,
    secret_service::{GetAllSecretsRequest, KeyValuePair},
};
use artisan_middleware::dusa_collection_utils::core::errors::{ErrorArrayItem, Errors};

#[derive(Clone, Debug)]
pub struct SecretQuery {
    pub (crate) runner_id: String,
    pub (crate) enviornment_id: String,
    pub (crate) version: i64,
}

pub type AllSecrets = Vec<(String, Vec<u8>)>;

impl SecretQuery {
    // This way when we roll the hashing for the complex id's there's not alot to change
    pub fn new(runner_id: String, enviornment_id: String, version: Option<i64>) -> Self {
        let version = if let Some(val) = version { val } else { 0 };

        Self {
            runner_id,
            enviornment_id,
            version,
        }
    }

    pub async fn get_all(&self, mut client: SecretClient) -> Result<AllSecrets, ErrorArrayItem> {
        let request: GetAllSecretsRequest = GetAllSecretsRequest {
            runner_id: self.runner_id.clone(),
            environment_id: self.enviornment_id.clone(),
            version: self.version,
        };

        match client.get_all_secrets(request).await {
            Ok(data) => {
                let items: Vec<KeyValuePair> = data.vals;
                let mut result: AllSecrets = Vec::new();

                for item in items {
                    result.push((item.key, item.value));
                }

                Ok(result)
            }
            Err(err) => Err(ErrorArrayItem::new(Errors::ConnectionError, err.message())),
        }
    }

    // pub fn get_val(&self, _val: String) {
    //     todo!()
    // }
}

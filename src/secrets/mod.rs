// Importing the proto file
pub(self) mod secret_service {
    tonic::include_proto!("secret_service");
}

// Exporting stuff
mod secret_handler;

pub struct SecretQuery {
    runner_id: String,
    enviornment_id: String,
    version: i64,
}

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

    pub fn get_all(&self) {
        todo!()
    }

    pub fn get_val(&self, _val: String) {
        todo!()
    }
}

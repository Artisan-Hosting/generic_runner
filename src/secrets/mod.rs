// Importing the proto file
pub(self) mod secret_service {
    tonic::include_proto!("secret_service");
}

// Exporting stuff
mod secret_handler;
mod secret_functions;
pub use secret_functions::SecretQuery;
pub use secret_handler::SecretClient;
use std::net::SocketAddr;
use std::{collections::BTreeMap, num::NonZeroU32};

use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};
pub use shuttle_common::{
    database,
    deployment::{DeploymentMetadata, Environment},
    project::ProjectName,
    resource::Type,
    DatabaseReadyInfo, DbInput, DbOutput, SecretStore,
};

pub mod error;
pub use error::{CustomError, Error};

#[cfg(feature = "builder")]
pub mod builder;

/// Factories can be used to request the provisioning of additional resources (like databases).
///
/// An instance of factory is passed by the deployer as an argument to [ResourceBuilder::output] in the initial phase of deployment.
///
/// Also see the [shuttle_runtime::main] macro.
#[async_trait]
pub trait Factory: Send + Sync {
    /// Get a database connection
    async fn get_db_connection(
        &mut self,
        db_type: database::Type,
    ) -> Result<DatabaseReadyInfo, crate::Error>;

    /// Get all the secrets for a service
    async fn get_secrets(&mut self) -> Result<BTreeMap<String, String>, crate::Error>;

    /// Get the metadata for this deployment
    fn get_metadata(&self) -> DeploymentMetadata;
}

/// Used to get resources of type `T` from factories.
///
/// This is mainly meant for consumption by our code generator and should generally not be called by users.
///
/// ## Creating your own managed resource
///
/// You may want to create your own managed resource by implementing this trait for some builder `B` to construct resource `T`.
/// [`Factory`] can be used to provision resources on Shuttle's servers if your service will need any.
///
/// Please refer to `shuttle-examples/custom-resource` for examples of how to create custom resource. For more advanced provisioning
/// of custom resources, please [get in touch](https://discord.gg/shuttle) and detail your use case. We'll be interested to see what you
/// want to provision and how to do it on your behalf on the fly.
///
/// ```
#[async_trait]
pub trait ResourceBuilder<T> {
    /// The type of resource this creates
    const TYPE: Type;

    /// The internal config being constructed by this builder. This will be used to find cached [Self::Output].
    type Config: Serialize;

    /// The output type used to build this resource later
    type Output: Serialize + DeserializeOwned;

    /// Create a new instance of this resource builder
    fn new() -> Self;

    /// Get the internal config state of the builder
    ///
    /// If the exact same config was returned by a previous deployement that used this resource, then [Self::output()]
    /// will not be called to get the builder output again. Rather the output state of the previous deployment
    /// will be passed to [Self::build()].
    fn config(&self) -> &Self::Config;

    /// Get the config output of this builder
    ///
    /// This method is where the actual resource provisioning should take place and is expected to take the longest. It
    /// can at times even take minutes. That is why the output of this method is cached and calling this method can be
    /// skipped as explained in [Self::config()].
    async fn output(self, factory: &mut dyn Factory) -> Result<Self::Output, crate::Error>;

    /// Build this resource from its config output
    async fn build(build_data: &Self::Output) -> Result<T, crate::Error>;
}

pub enum Idle {
    DoIdle(NonZeroU32),
    AlwaysOn,
}

/// The core trait of the Shuttle platform. Every crate deployed to Shuttle needs to implement this trait.
///
/// Use the [shuttle_runtime::main] macro to expose your implementation to the deployment backend.
#[async_trait]
pub trait Service: Send + Clone {
    const IDLE: Idle = Idle::DoIdle(unsafe { NonZeroU32::new_unchecked(30) });

    /// This function is run on startup after loading the service.
    ///
    /// The service can bind to the passed [SocketAddr][SocketAddr] if desired.
    async fn bind(self, addr: SocketAddr) -> Result<(), error::Error>;

    /// This is called after startup to check if the service is healthy.
    ///
    /// Default implementation assumes the service is bound to `addr` and responds with 200 OK on '/_shuttle/healthz'.
    /// Override this if not relevant.
    async fn health_check(self, addr: &SocketAddr) -> Result<(), error::Error> {
        reqwest::get(reqwest::Url::parse(&format!("http://{addr}/_shuttle/healthz")).unwrap())
            .await
            .map_err(|e| Error::HeathCheckFailed(e.to_string()))?
            .status()
            .is_success()
            .then(|| ())
            .ok_or(Error::HeathCheckFailed("Health check unsuccessful".into()))
    }

    /// Called before shutdown of this service happens. Gives time for service to do graceful shutdown.
    async fn shutdown(self) -> Result<(), error::Error> {
        Ok(())
    }
}

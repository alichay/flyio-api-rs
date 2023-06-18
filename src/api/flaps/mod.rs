use std::{fmt::Display, time::Duration, sync::Arc};

use thiserror::Error;

mod types;
pub use types::*;
use crate::entities;

mod transport;
use transport::*;
#[cfg(feature = "unix-socket")]
mod unix;

pub type Result<T> = std::result::Result<T, FlapsError>;

#[derive(Error, Debug)]
pub enum FlapsClientCreationError {
    #[error("Missing app name")]
    MissingAppName,
    #[error("Invalid app name")]
    InvalidAppName(String),
    #[error("Invalid base url: {0}")]
    InvalidBaseUrl(#[from] url::ParseError),
}

#[derive(serde::Deserialize, Debug)]
pub struct RawApiError {
    pub status_code: u16,
    pub fly_request_id: Option<String>,
    pub error: String,
    pub message: Option<String>,
}

impl Display for RawApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.error)?;
        if let Some(msg) = &self.message {
            write!(f, ": {}", msg)?
        }
        Ok(())
    }
}

#[derive(Error, Debug)]
pub enum FlapsError {
    #[error("Invalid endpoint: {0}")]
    InvalidUrl(#[from] url::ParseError),
    #[error("HTTP error: {0}")]
    Http(#[from] http::Error),
    #[error("HTTP error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("Socket transport error: {0}")]
    Hyper(#[from] hyper::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Unexpected HTTP status code {0}")]
    UnexpectedHttpStatus(reqwest::StatusCode),

    #[error("Unknown flaps error: {0}")]
    UnknownFlapsError(RawApiError),

    #[error("No Machine ID provided")]
    NoMachineId,

    #[error("Not found")]
    NotFound(RawApiError),

    #[error("Timed out waiting for machine to reach desired state '{desired_state}'")]
    DesiredStateNotReached{desired_state: crate::entities::machine::State, raw: RawApiError},
}

// Used to determine the behavior of [`map_flaps_error`]
enum ApiEndpoint {
    Other,
    Wait(crate::entities::machine::State),
}

fn map_flaps_error(body: bytes::Bytes, fly_request_id: Option<String>, status: reqwest::StatusCode, endpoint: ApiEndpoint) -> FlapsError {

    if status.is_success() {
        unreachable!("map_flaps_error called with success status code")
    }
    if status.is_informational() || status.is_redirection() {
        return FlapsError::UnexpectedHttpStatus(status);
    }
    if !status.is_client_error() && !status.is_server_error() {
        // Invalid response
        return FlapsError::UnexpectedHttpStatus(status);
    }



    let status_code = status.as_u16();

    let raw_api_err: RawApiError = serde_json::from_slice(&body).unwrap_or_else(|_| RawApiError {
        error: format!("Server returned non-2xx status code {}, raw response: {:?}", status, body),
        message: None,
        fly_request_id,
        status_code,
    });

    // Attempt to use string comparison to map errors to strong types

    match endpoint {
        ApiEndpoint::Wait(desired_state) => {
            if status == http::StatusCode::REQUEST_TIMEOUT {
                return FlapsError::DesiredStateNotReached{desired_state, raw: raw_api_err};
            }
        },
        ApiEndpoint::Other => {},
    }


    if status == reqwest::StatusCode::NOT_FOUND {
        return FlapsError::NotFound(raw_api_err);
    }
    

    FlapsError::UnknownFlapsError(raw_api_err)
}

pub struct FlapsSettings {
    base_url: Option<String>,
    user_agent: Option<String>,
    auth_token: String,
    app_name: Option<String>,
}

struct RawClient {
    client: Transport,
    // base_url: reqwest::Url,
    app_url: url::Url,
    user_agent: String,
    // app_name: String,
}

pub struct Client(Arc<RawClient>);

fn default_base_url() -> String {
    match std::env::var("FLY_FLAPS_BASE_URL") {
        Ok(url) => url,
        Err(_) => {
            match crate::api::env::running_on_fly() {
                true => "http://_api.internal:4280".to_string(),
                false => "https://api.machines.dev".to_string(),
            }
        }
    }
}

const PROXY_TIMEOUT_THRESHOLD: Duration = Duration::from_secs(60);

pub(crate) struct HeaderPair(&'static str, String);
impl HeaderPair {
    fn lease_nonce(nonce: String) -> HeaderPair {
        HeaderPair("fly-machine-lease-nonce", nonce)
    }
}
fn add_lease_nonce(headers: &mut Vec<HeaderPair>, nonce: Option<String>) {
    if let Some(nonce) = nonce {
        headers.push(HeaderPair::lease_nonce(nonce));
    }
}
struct UrlParam<'a, 'b>(&'a str, &'b str);
fn encode_url_params(params: &[UrlParam]) -> String {
    if params.is_empty() {
        return String::new();
    }
    let mut pairs = Vec::with_capacity(params.len());
    for param in params {
        pairs.push(format!("{}={}", urlencoding::encode(param.0), urlencoding::encode(param.1)));
    }
    "?".to_string() + &pairs.join("&")
}

impl Client {

    pub fn new(cfg: FlapsSettings) -> std::result::Result<Client, FlapsClientCreationError> {

        let base_url = reqwest::Url::parse(&cfg.base_url.unwrap_or_else(default_base_url))?;
        let app_name = cfg.app_name.or_else(crate::api::env::current_app_name).ok_or(FlapsClientCreationError::MissingAppName)?;

        if app_name.chars().any(|c| matches!(c, '/'|':'|'\\')) {
            return Err(FlapsClientCreationError::InvalidAppName(app_name));
        }

        let auth_header = if cfg.auth_token.starts_with("FlyV1 ") {
            cfg.auth_token
        } else {
            format!("Bearer {}", cfg.auth_token)
        };

        Ok(Client(Arc::new(RawClient {
            client: HttpTransport(reqwest::Client::new(), auth_header).into(),
            app_url: base_url.join(format!("v1/apps/{}/machines", &app_name).as_str())?,
            // base_url,
            user_agent: cfg.user_agent.unwrap_or_else(|| format!("flyio-api-rs/{}", env!("CARGO_PKG_VERSION"))),
            // app_name,
        })))
    }

    #[cfg(feature = "unix-socket")]
    // TODO: Test this!
    pub fn new_from_socket(app_name: Option<String>) -> std::result::Result<Client, FlapsClientCreationError> {
        let app_name = app_name.or_else(crate::api::env::current_app_name).ok_or(FlapsClientCreationError::MissingAppName)?;
        // Hostname unused, just has to exist. We use the unix socket to route.
        let base_url = url::Url::parse("http://localhost").unwrap();

        if app_name.chars().any(|c| matches!(c, '/'|':'|'\\')) {
            return Err(FlapsClientCreationError::InvalidAppName(app_name));
        }
        let client = hyper::Client::builder().build(unix::UnixSocketConnector);

        Ok(Client(Arc::new(RawClient {
            client: UnixSocketTransport(client).into(),
            app_url: base_url.join(format!("v1/apps/{}/machines", &app_name).as_str())?,
            // base_url,
            user_agent: format!("flyio-api-rs-unix/{}", env!("CARGO_PKG_VERSION")),
            // app_name,
        })))
    }

    async fn make_request_raw(&self, method: reqwest::Method, url: reqwest::Url, json: String, headers: Vec<HeaderPair>, api_endpoint: ApiEndpoint) -> Result<bytes::Bytes> {

        let res = self.0.client.make_request(&self.0.user_agent, method, url, json, headers).await?;
        let TransportResult {body, status_code, request_id} = res;

        if status_code.as_u16() > 299 {
            return Err(map_flaps_error(body, request_id, status_code, api_endpoint));
        }

        Ok(body)
    }

    async fn make_machines_request<
        Res: serde::de::DeserializeOwned,
        Req: serde::Serialize,
    >(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        data: Req,
        headers: Vec<HeaderPair>,
        api_endpoint: ApiEndpoint,
    ) -> Result<Res> {

        let json = serde_json::to_string(&data)?;
        let bytes = self.make_request_raw(method, self.0.app_url.join(endpoint)?, json, headers, api_endpoint).await?;
        let response = serde_json::from_slice(&bytes)?;
        Ok(response)
    }

    async fn make_machines_request_into<
        Res: serde::de::DeserializeOwned,
        Req: serde::Serialize,
    >(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        data: Req,
        res: &mut Res,
        headers: Vec<HeaderPair>,
        api_endpoint: ApiEndpoint,
    ) -> Result<()> {

        let json = serde_json::to_string(&data)?;
        let bytes = self.make_request_raw(method, self.0.app_url.join(endpoint)?, json, headers, api_endpoint).await?;
        let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
        Res::deserialize_in_place(&mut deserializer, res)?;
        Ok(())
    }

    pub async fn launch(&self, req: LaunchMachineInput) -> Result<entities::machine::Machine> {
        self.make_machines_request(reqwest::Method::POST, "", req, Vec::new(), ApiEndpoint::Other).await
    }
    pub async fn update(&self, req: LaunchMachineInput, nonce: Option<String>) -> Result<entities::machine::Machine> {
        let mut headers = Vec::new();
        add_lease_nonce(&mut headers, nonce);

        self.make_machines_request(reqwest::Method::POST, req.id.as_ref().ok_or(FlapsError::NoMachineId)?.as_str(), &req, headers, ApiEndpoint::Other).await
    }
    pub async fn start<M: AsMachineId>(&self, machine: M, nonce: Option<String>) -> Result<MachineStartResponse> {
        let mut headers = Vec::new();
        add_lease_nonce(&mut headers, nonce);

        let machine_id = machine.as_machine_id();

        self.make_machines_request(reqwest::Method::POST, &format!("{machine_id}/start"), (), headers, ApiEndpoint::Other).await
    }
    /// Waits for a machine to reach a state.
    /// The timeout is clamped between one second and [`PROXY_TIMEOUT_THRESHOLD`].
    /// If you want a longer timeout, use [`wait_for_state`], which calls this method repeatedly until the timeout is reached or the machine reaches the desired state.
    pub async fn wait(&self, machine: &entities::machine::Machine, state: Option<entities::machine::State>, timeout: Duration) -> Result<()> {

        let machine_id = &machine.id;

        let state = state.unwrap_or(entities::machine::State::Started);
        let mut version: &str = &machine.instance_id;
        if let Some(ver) = machine.version.as_deref() {
            version = ver;
        }
        let timeout = timeout.clamp(Duration::from_secs(1), PROXY_TIMEOUT_THRESHOLD);
        let timeout_secs = timeout.as_secs();

        let wait_query = encode_url_params(&[
            UrlParam("instance_id", version),
            UrlParam("state", &state.name()),
            UrlParam("timeout", &timeout_secs.to_string()),
        ]);

        self.make_machines_request(reqwest::Method::GET, &format!("{machine_id}/wait{wait_query}"), (), Vec::new(), ApiEndpoint::Wait(state)).await
        
    }

    pub async fn wait_for_state(&self, machine: &entities::machine::Machine, state: Option<entities::machine::State>, timeout: Duration) -> Result<()> {
        let n = backoff::ExponentialBackoffBuilder::new()
            .with_initial_interval(Duration::from_millis(500))
            .with_max_interval(Duration::from_millis(5000))
            .with_multiplier(1.5)
            .with_max_elapsed_time(Some(timeout))
            .build();

        let end_time = std::time::Instant::now() + timeout + Duration::from_millis(100);

        backoff::future::retry(n, || async {

            let current_time = std::time::Instant::now();
            let time_left = (end_time - current_time).min(Duration::from_secs(2));

            match self.wait(machine, state.clone(), time_left).await {
                Ok(_) => Ok(()),
                Err(e) => {
                    if matches!(e, FlapsError::DesiredStateNotReached{..}) {
                        Err(backoff::Error::Transient{
                            err: e,
                            retry_after: None,
                        })
                    } else {
                        Err(backoff::Error::Permanent(e))
                    }
                }
            }
        }).await
    }


    pub async fn stop(&self, stop_input: StopMachineInput, nonce: Option<String>) -> Result<()> {
        let mut headers = Vec::new();
        add_lease_nonce(&mut headers, nonce);

        self.make_machines_request(reqwest::Method::POST, &format!("{}/stop", stop_input.id), stop_input, headers, ApiEndpoint::Other).await
    }

    pub async fn restart(&self, restart_input: RestartMachineInput, nonce: Option<String>) -> Result<()> {

        let machine_id = &restart_input.id;

        let mut headers = Vec::new();
        add_lease_nonce(&mut headers, nonce);

        let force_stop_str = restart_input.force_stop.to_string();
        let timeout_str = restart_input.timeout.map(|t| t.as_nanos().to_string());
        let mut url_params = vec![
            UrlParam("force_stop", &force_stop_str),
        ];
        if let Some(timeout_str) = timeout_str.as_ref() {
            url_params.push(UrlParam("timeout", timeout_str));
        }
        if let Some(signal) = restart_input.signal.as_ref() {
            url_params.push(UrlParam("signal", signal));
        }
        let restart_query = encode_url_params(&url_params);

        self.make_machines_request(reqwest::Method::POST, &format!("{machine_id}/restart{restart_query}"), (), headers, ApiEndpoint::Other).await
    }

    pub async fn get<M: AsMachineId>(&self, machine: &M) -> Result<entities::machine::Machine> {
        let machine_id = machine.as_machine_id();

        self.make_machines_request(reqwest::Method::GET, machine_id, (), Vec::new(), ApiEndpoint::Other).await
    }

    pub async fn get_many<M: AsMachineId>(&self, machine_ids: &[M]) -> Result<Vec<entities::machine::Machine>> {
        let futures = machine_ids.iter().map(|id| {
            self.get(id)
        });
        futures::future::try_join_all(futures).await
    }

    pub async fn list(&self, state: Option<&str>) -> Result<Vec<entities::machine::Machine>> {
        self.make_machines_request(
            reqwest::Method::GET,
            &state.map(|s| "?".to_string() + s).unwrap_or_default(),
            (),
            Vec::new(),
            ApiEndpoint::Other
        ).await
    }
    #[doc(hidden)]
    pub async fn list_into(&self, vec: &mut Vec<entities::machine::Machine>, state: Option<&str>) -> Result<()> {
        self.make_machines_request_into(
            reqwest::Method::GET,
            &state.map(|s| "?".to_string() + s).unwrap_or_default(),
            (),
            vec,
            Vec::new(),
            ApiEndpoint::Other
        ).await
    }

    /// returns only non-destroyed machines that aren't in a reserved process group
    pub async fn list_active(&self) -> Result<Vec<entities::machine::Machine>> {
        let mut machs = self.list(None).await?;
        machs.retain(|m| {
            !m.is_release_command_machine() && !m.is_fly_apps_console() && m.is_active()
        });
        Ok(machs)
    }
    #[doc(hidden)]
    pub async fn list_active_into(&self, vec: &mut Vec<entities::machine::Machine>) -> Result<()> {
        self.list_into(vec, None).await?;
        vec.retain(|m| {
            !m.is_release_command_machine() && !m.is_fly_apps_console() && m.is_active()
        });
        Ok(())
    }

    /// returns machines that are part of the fly apps platform that are not destroyed, excluding console machines
    pub async fn list_fly_apps_machines(&self) -> Result<FlyAppsMachines> {
        let n = backoff::ExponentialBackoffBuilder::new()
            .with_initial_interval(Duration::from_millis(500))
            .with_max_interval(Duration::from_millis(5000))
            .with_multiplier(1.5)
            .build();

        backoff::future::retry(n, || async {

            let mut machs = match self.list(None).await {
                Ok(machs) => machs,
                Err(e) => {
                    if matches!(e, FlapsError::NotFound(_)) {
                        return Err(backoff::Error::Transient{
                            err: e,
                            retry_after: None,
                        });
                    } else {
                        return Err(backoff::Error::Permanent(e));
                    }
                }
            };

            let release_cmd_mach = machs.iter().find(|m| m.is_release_command_machine()).cloned();

            machs.retain(|m| {
                !m.is_release_command_machine() && !m.is_fly_apps_console()
            });
            Ok(FlyAppsMachines {
                machines: machs,
                release_cmd_machine: release_cmd_mach,
            })
        }).await
    }

    pub async fn destroy(&self, input: RemoveMachineInput, nonce: Option<String>) -> Result<()> {
        let mut headers = Vec::new();
        add_lease_nonce(&mut headers, nonce);

        let kill = match input.kill {
            true => "true",
            false => "false",
        };

        self.make_machines_request(reqwest::Method::DELETE, &format!("{}/destroy?kill={kill}", input.id), (), headers, ApiEndpoint::Other).await
    }

    pub async fn kill<M: AsMachineId>(&self, machine: M) -> Result<()> {
        let machine_id = machine.as_machine_id();

        self.make_machines_request(reqwest::Method::POST, &format!("{}/signal", machine_id), Signal::from(9), Vec::new(), ApiEndpoint::Other).await
    }

    pub async fn find_lease<M: AsMachineId>(&self, machine: M) -> Result<Option<MachineLease>> {
        let machine_id = machine.as_machine_id();

        let res = self.make_machines_request(
            reqwest::Method::GET,
            &format!("{}/lease", machine_id),
            (),
            Vec::new(),
            ApiEndpoint::Other,
        ).await;

        match res {
            Ok(lease) => Ok(Some(lease)),
            Err(FlapsError::NotFound(raw)) => {
                if raw.message.as_deref() == Some("lease not found") {
                    Ok(None)
                } else {
                    Err(FlapsError::NotFound(raw))
                }
            },
            Err(e) => Err(e),
        }
    }

    pub async fn acquire_lease<M: AsMachineId>(&self, machine: M, ttl: Option<i32>) -> Result<MachineLease> {
        let machine_id = machine.as_machine_id();

        let mut url_params = Vec::new();
        let ttl = ttl.as_ref().map(ToString::to_string);
        if let Some(ttl) = ttl.as_deref() {
            url_params.push(UrlParam("ttl", ttl));
        }
        let lease_query = encode_url_params(&url_params);

        self.make_machines_request(reqwest::Method::POST, &format!("{}/lease{lease_query}", machine_id), (), Vec::new(), ApiEndpoint::Other).await
    }

    pub async fn refresh_lease<M: AsMachineId>(&self, machine: M, ttl: Option<i32>, nonce: String) -> Result<MachineLease> {
        let machine_id = machine.as_machine_id();

        let headers = vec![HeaderPair::lease_nonce(nonce)];

        let mut url_params = Vec::new();
        let ttl = ttl.as_ref().map(ToString::to_string);
        if let Some(ttl) = ttl.as_deref() {
            url_params.push(UrlParam("ttl", ttl));
        }
        let lease_query = encode_url_params(&url_params);

        self.make_machines_request(reqwest::Method::POST, &format!("{}/lease/refresh{lease_query}", machine_id), (), headers, ApiEndpoint::Other).await
    }

    pub async fn release_lease<M: AsMachineId>(&self, machine: M, nonce: Option<String>) -> Result<()> {
        let machine_id = machine.as_machine_id();

        let mut headers = Vec::new();
        add_lease_nonce(&mut headers, nonce);

        self.make_machines_request(reqwest::Method::DELETE, &format!("{}/lease", machine_id), (), headers, ApiEndpoint::Other).await
    }

    pub async fn exec<M: AsMachineId>(&self, machine: M, input: MachineExecRequest) -> Result<MachineExecResponse> {
        let machine_id = machine.as_machine_id();

        self.make_machines_request(reqwest::Method::POST, &format!("{}/exec", machine_id), input, Vec::new(), ApiEndpoint::Other).await
    }

    pub async fn get_processes<M: AsMachineId>(&self, machine: M) -> Result<Vec<crate::entities::ProcessStat>> {
        let machine_id = machine.as_machine_id();

        self.make_machines_request(reqwest::Method::GET, &format!("{}/ps", machine_id), (), Vec::new(), ApiEndpoint::Other).await
    }
}
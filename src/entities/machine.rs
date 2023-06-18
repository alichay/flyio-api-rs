use std::{
    borrow::Cow,
    collections::{HashMap, BTreeMap}, hash::Hash, fmt::{Display, Formatter},
};

use phf::phf_map;
use thiserror::Error;
use super::GoTime;


#[derive(Debug, serde::Deserialize, Clone)]
pub struct Machine {
    pub id: String,
    pub name: String,
    pub state: State,
    pub region: String,
    pub image_ref: ImageRef,
    /// InstanceID is unique for each version of the machine
    pub instance_id: String,
    pub version: Option<String>,
    /// PrivateIP is the internal 6PN address of the machine.
    pub private_ip: String,
    pub created_at: GoTime,
    pub updated_at: GoTime,
    pub config: Option<Config>,
    pub events: Vec<MachineEvent>,
    pub checks: Vec<CheckStatus>,
    #[serde(rename = "nonce")]
    pub lease_nonce: String,
}

pub const MACHINE_CONFIG_METADATA_KEY_FLY_PLATFORM_VERSION: &str = "fly_platform_version";
pub const MACHINE_FLY_PLATFORM_VERSION_2: &str = "v2";

pub const MACHINE_CONFIG_METADATA_KEY_FLY_PROCESS_GROUP: &str = "fly_process_group";
pub const MACHINE_PROCESS_GROUP_FLY_APP_RELEASE_COMMAND: &str = "fly_app_release_command";
pub const MACHINE_PROCESS_GROUP_APP: &str = "app";
pub const MACHINE_PROCESS_GROUP_FLY_APP_CONSOLE: &str = "fly_app_console";

impl Machine {
    fn with_metadata<T, F: FnOnce(&BTreeMap<String, String>) -> T>(&self, f: F) -> Option<T> {
        self.config.as_ref().and_then(|c| c.metadata.as_ref().map(f))
    }
    pub fn map_process_group<R, F: FnOnce(&str) -> R>(&self, f: F) -> Option<R> {
        self.with_metadata(|m| {
            if let Some(fpg) = m.get(MACHINE_CONFIG_METADATA_KEY_FLY_PROCESS_GROUP) {
                return Some(f(fpg))
            }
            m.get("process_group").map(String::as_str).map(f)
        }).flatten()
    }
    pub fn process_group(&self) -> Option<String> {
        self.map_process_group(|p| p.to_string())
    }
    pub fn has_process_group(&self, group: &str) -> bool {
        self.map_process_group(|p| p == group).unwrap_or(false)
    }
    pub fn has_any_process_group<S: AsRef<str>>(&self, groups: &[S]) -> bool {
        self.map_process_group(|p| groups.iter().any(|g| g.as_ref() == p)).unwrap_or(false)
    }
    pub fn is_release_command_machine(&self) -> bool {
        self.has_any_process_group(&[
            // Modern
            MACHINE_PROCESS_GROUP_FLY_APP_RELEASE_COMMAND,
            // Legacy
            "release_command",
        ])
    }
    pub fn is_apps_v2(&self) -> bool {
        self.with_metadata(|m| {
            if let Some(fpv) = m.get(MACHINE_CONFIG_METADATA_KEY_FLY_PLATFORM_VERSION) {
                return fpv == MACHINE_FLY_PLATFORM_VERSION_2
            }
            false
        }).unwrap_or(false)
    }
    pub fn is_active(&self) -> bool {
        self.state != State::Destroyed && self.state != State::Destroying
    }
    pub fn is_fly_apps_platform(&self) -> bool {
        self.is_apps_v2() && self.is_active()
    }
    pub fn is_fly_apps_console(&self) -> bool {
        self.is_fly_apps_platform() && self.has_process_group(MACHINE_PROCESS_GROUP_FLY_APP_CONSOLE)
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone, PartialEq)]
pub struct ImageRef {
    pub registry: String,
    pub repository: String,
    pub tag: Option<String>,
    pub digest: Option<String>,
    pub labels: HashMap<String, String>,
}

impl ImageRef {
    pub fn full_ref(&self) -> String {
        let mut img_ref = format!("{}/{}", self.registry, self.repository);
        if let Some(tag) = &self.tag {
            img_ref.push(':');
            img_ref.push_str(tag);
        }
        if let Some(digest) = &self.digest {
            img_ref.push('@');
            img_ref.push_str(digest);
        }
        img_ref
    }
    // Format as "repository:tag (fly.version)" (or just "repository:tag" if no fly.version label is present)
    pub fn str_with_version(&self) -> String {
        let mut img_ref = format!("{}:{}", self.repository, self.tag.as_deref().unwrap_or(""));
        if let Some(ver) = self.labels.get("fly.version") {
            img_ref.push_str(" (");
            img_ref.push_str(ver);
            img_ref.push(')');
        }
        img_ref
    }
    // TODO: Validate that this works the way I expect it - this is entirely untested.
    // It's meant to compare an ImageRef to MachineConfig.image, but I'm not sure this is the exact format that that should be.
    pub fn matches_str(&self, s: &str) -> bool {
        let mut s = s;

        // TODO: Might want some sort of validation for default registry?
        // As it stands, if a machine is running myregistry.com/foobar:latest,
        // and I try to compare against "foobar:latest", it will match.

        'img_name: {
            if s.starts_with(&self.repository) {
                s = &s[self.repository.len()..];
                break 'img_name
            }

            if s.starts_with(&self.registry) {
                s = &s[self.registry.len()..];
                if s.starts_with('/') {
                    s = &s[1..];
                }
                if s == self.repository {
                    s = &s[self.repository.len()..];
                    break 'img_name
                }
            }
            return false
        }

        'tag: {
            if s.starts_with(':') {

                if let Some(tag) = &self.tag {
                    s = &s[1..];
                    if s.starts_with(tag) {
                        s = &s[tag.len()..];
                        break 'tag
                    }
                }
                return false
            }
        }

        'digest: {
            if s.starts_with('@') {
                if let Some(digest) = &self.digest {
                    s = &s[1..];
                    if s.starts_with(digest) {
                        s = &s[digest.len()..];
                        break 'digest
                    }
                }
                return false
            }
        }

        s.is_empty()
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone)]
pub struct MachineEvent {
    #[serde(rename = "type")]
    pub type_: String,
    pub status: String,
    pub request: Option<MachineRequest>, // TODO: Is this optional?
    pub source: String,
    pub timestamp: i64,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone)]
pub struct MachineRequest {
    pub exit_event: Option<MachineExitEvent>, // TODO: Are these optional?
    pub monitor_event: Option<MachineMonitorEvent>,
    pub restart_count: i32,
}

#[derive(Debug, Error)]
#[error("No exit code in this MachineRequest")]
pub struct NoExitCodeError;

impl MachineRequest {
    pub fn get_exit_code(&self) -> Result<i32, NoExitCodeError> {
        if let Some(me) = &self.monitor_event {
            if let Some(ee) = &me.exit_event {
                return Ok(ee.exit_code);
            }
        }
        if let Some(ee) = &self.exit_event {
            Ok(ee.exit_code)
        } else {
            Err(NoExitCodeError)
        }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone)]
pub struct MachineMonitorEvent {
    pub exit_event: Option<MachineExitEvent>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone)]
pub struct MachineExitEvent {
    pub exit_code: i32,
    pub guest_exit_code: i32,
    pub guest_signal: i32,
    pub oom_killed: bool,
    pub requested_stop: bool,
    pub restarting: bool,
    pub signal: i32,
    pub exited_at: Option<super::GoTime>,
}

#[repr(C)]
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum ConsulCheckStatus {
    #[serde(rename = "critical")]
    Critical,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "passing")]
    Passing,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
pub struct CheckStatus {
    pub name: String,
    pub status: ConsulCheckStatus,
    pub output: String,
    pub updated_at: Option<super::GoTime>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone, Hash)]
pub struct Config {

    pub env: Option<std::collections::BTreeMap<String, String>>,
    pub init: Option<Init>,
    pub metadata: Option<std::collections::BTreeMap<String, String>>,
    pub mounts: Vec<Mount>,
    pub services: Vec<Service>,
    pub metrics: Option<Metrics>,
    pub checks: Option<std::collections::BTreeMap<String, Check>>,
    pub statics: Vec<Static>,

    pub image: String,

    pub schedule: Option<String>,
    pub auto_destroy: bool,
    pub restart: Restart,
    pub guest: Option<Guest>,
    pub dns: Option<DNSConfig>,
    pub processes: Vec<Process>,

    pub standbys: Vec<String>,

    pub stop_config: Option<StopConfig>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone, Hash)]
pub struct Init {
    pub exec: Vec<String>,
    pub entrypoint: Vec<String>,
    pub cmd: Vec<String>,
    pub tty: bool,
}


#[derive(Debug, serde::Deserialize, serde::Serialize, Clone, Hash)]
pub struct Mount {
    pub encrypted: Option<bool>,
    pub path: String,
    pub size_gb: Option<i32>,
    volume: Option<String>,
    name: Option<String>,
}

impl Mount {
    fn private_default() -> Self {
        Self {
            encrypted: None,
            path: "".to_string(),
            size_gb: None,
            volume: None,
            name: None,
        }
    }
    pub fn from_vol_id(vol_id: String, path: String) -> Self {
        Self {
            volume: Some(vol_id),
            path,
            ..Self::private_default()
        }
    }
    pub fn from_vol_name(vol_name: String, path: String) -> Self {
        Self {
            name: Some(vol_name),
            path,
            ..Self::private_default()
        }
    }
    pub fn vol_id(&self) -> Option<&str> {
        self.volume.as_deref()
    }
    pub fn vol_name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone, Hash)]
pub struct Service {
    pub protocol: String,
    pub internal_port: i32,
    pub autostop: Option<bool>,
    pub autostart: Option<bool>,
    pub min_machines_running: Option<i32>,
    pub ports: Vec<Port>,
    pub checks: Vec<Check>,
    pub concurrency: Option<ServiceConcurrency>,
    // force_instance_key: String,
    // force_instance_description: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone, Hash)]
pub struct Metrics {
    pub port: i32,
    pub path: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone, Hash)]
pub struct Check {
    pub port: Option<i32>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub interval: Option<super::FlyctlDuration>,
    pub timeout: Option<super::FlyctlDuration>,
    pub grace_period: Option<super::FlyctlDuration>,
    #[serde(rename = "method")]
    pub http_method: Option<String>,
    #[serde(rename = "path")]
    pub http_path: Option<String>,
    #[serde(rename = "protocol")]
    pub http_protocol: Option<String>,
    #[serde(rename = "tls_skip_verify")]
    pub http_skip_tls_verify: Option<bool>,
    #[serde(rename = "headers")]
    pub http_headers: Vec<HttpHeader>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone, Hash)]
pub struct Static {
    pub guest_path: String,
    pub url_prefix: String,
}

#[repr(C)]
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone, Copy, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum RestartPolicy {
    No,
    OnFailure,
    Always,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self::No
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone, Hash)]
pub struct Restart {
    pub policy: Option<RestartPolicy>,
    pub max_retries: i32,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone, Hash)]
pub struct Guest {
    pub cpu_kind: Cow<'static, str>,
    pub cpus: i32,
    pub memory_mb: i32,
    pub kernel_args: Vec<String>,
}

#[derive(Debug, Error)]
pub enum SetSizeError {
    #[error("invalid machine preset requested '{size}', expected to start with 'shared' or 'performance'")]
    InvalidPreset { size: String },
    #[error("{size} is an invalid machine size, choose one of [{valid_sizes}]")]
    InvalidSize { size: String, valid_sizes: String},
}

impl Guest {

    pub fn from_size(size: &str) -> Result<Self, SetSizeError> {
        let mut guest = Self::default();
        guest.set_size(size)?;
        Ok(guest)
    }

    pub fn set_size(&mut self, size: &str) -> Result<(), SetSizeError> {

        if let Some(guest) = MACHINE_PRESETS.get(size) {
            self.cpus = guest.cpus;
            self.cpu_kind = guest.cpu_kind.clone();
            self.memory_mb = guest.memory_mb;
            return Ok(())
        }
    
        let machine_type = if size.starts_with("shared") {
            "shared"
        } else if size.starts_with("performance") {
            "performance"
        } else {
            return Err(SetSizeError::InvalidPreset { size: size.to_string() });
        };

        let mut potential_sizes: Vec<_> = MACHINE_PRESETS
            .into_iter()
            .flat_map(|(name, _)| {
                if name.starts_with(machine_type) {
                    Some(*name)
                } else {
                    None
                }
            }).collect::<Vec<_>>();
        potential_sizes.sort();

        Err(SetSizeError::InvalidSize {
            size: size.to_string(),
            valid_sizes: potential_sizes.join(", "),
        })
    }

    pub fn to_size_type(&self) -> String {
        let cpus = self.cpus;
        match self.cpu_kind.as_ref() {
            "shared" => format!("shared-cpu-{cpus}x"),
            "dedicated" => format!("dedicated-cpu-{cpus}x"),
            _ => "unknown".to_string(),
        }
    }
}

pub const MIN_MEMORY_MB_PER_SHARED_CPU: i32 = 256;
pub const MIN_MEMORY_MB_PER_CPU:        i32 = 2048;

pub const MAX_MEMORY_MB_PER_SHARED_CPU: i32 = 2048;
pub const MAX_MEMORY_MB_PER_CPU:        i32 = 8192;

// TODO - Determine if we want allocate max memory allocation, or minimum per # cpus.
#[allow(clippy::identity_op)]
pub const MACHINE_PRESETS: phf::Map<&'static str, Guest> = phf_map!{
    "shared-cpu-1x" => Guest {cpu_kind: Cow::Borrowed("shared"), cpus: 1, memory_mb: 1 * MIN_MEMORY_MB_PER_SHARED_CPU, kernel_args: Vec::new()},
    "shared-cpu-2x" => Guest {cpu_kind: Cow::Borrowed("shared"), cpus: 2, memory_mb: 2 * MIN_MEMORY_MB_PER_SHARED_CPU, kernel_args: Vec::new()},
    "shared-cpu-4x" => Guest {cpu_kind: Cow::Borrowed("shared"), cpus: 4, memory_mb: 4 * MIN_MEMORY_MB_PER_SHARED_CPU, kernel_args: Vec::new()},
    "shared-cpu-8x" => Guest {cpu_kind: Cow::Borrowed("shared"), cpus: 8, memory_mb: 8 * MIN_MEMORY_MB_PER_SHARED_CPU, kernel_args: Vec::new()},

    "performance-1x" => Guest {cpu_kind: Cow::Borrowed("performance"), cpus: 1, memory_mb: 1 * MIN_MEMORY_MB_PER_CPU, kernel_args: Vec::new()},
    "performance-2x" => Guest {cpu_kind: Cow::Borrowed("performance"), cpus: 2, memory_mb: 2 * MIN_MEMORY_MB_PER_CPU, kernel_args: Vec::new()},
    "performance-4x" => Guest {cpu_kind: Cow::Borrowed("performance"), cpus: 4, memory_mb: 4 * MIN_MEMORY_MB_PER_CPU, kernel_args: Vec::new()},
    "performance-8x" => Guest {cpu_kind: Cow::Borrowed("performance"), cpus: 8, memory_mb: 8 * MIN_MEMORY_MB_PER_CPU, kernel_args: Vec::new()},
    "performance-16x" => Guest {cpu_kind: Cow::Borrowed("performance"), cpus: 16,memory_mb: 16 * MIN_MEMORY_MB_PER_CPU, kernel_args: Vec::new()},
};

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone, Hash)]
pub struct DNSConfig {
    pub skip_registration: bool,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone, Hash)]
pub struct Process {
    #[serde(rename = "exec")]
    pub exec_override: Vec<String>,
    #[serde(rename = "entrypoint")]
    pub entrypoint_override: Vec<String>,
    #[serde(rename = "cmd")]
    pub cmd_override: Vec<String>,
    #[serde(rename = "user")]
    pub user_override: String,
    #[serde(rename = "env")]
    pub extra_env: BTreeMap<String, String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone, Hash)]
pub struct StopConfig {
    pub timeout: Option<super::FlyctlDuration>,
    pub signal: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone, Hash)]
pub struct Port {
    pub port: Option<u16>,
    pub start_port: Option<u16>,
    pub end_port: Option<u16>,
    pub handlers: Vec<String>,
    pub force_https: bool,
    pub tls_options: Option<TlsOptions>,
    pub http_options: Option<HttpOptions>,
    pub proxy_proto_options: Option<ProxyProtoOptions>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone, Hash)]
pub struct ServiceConcurrency {
    #[serde(rename = "type")]
    pub type_: String,
    pub hard_limit: i32,
    pub soft_limit: i32,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone, Hash)]
pub struct HttpHeader {
    pub name: String,
    pub value: Vec<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone, Hash)]
pub struct TlsOptions {
    pub alpn: Vec<String>,
    pub versions: Vec<String>,
    pub default_self_signed: Option<bool>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone, Hash)]
pub struct HttpOptions {
    pub compress: Option<bool>,
    pub response: Option<HttpResponseOptions>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone, Hash)]
pub struct ProxyProtoOptions {
    pub version: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Default, Clone)]
pub struct HttpResponseOptions {
    pub headers: std::collections::BTreeMap<String, serde_json::Value>,
}
fn hash_jsvalue<H: std::hash::Hasher>(v: &serde_json::Value, state: &mut H) {
    match v {
        serde_json::Value::Null => {
            state.write_u8(0);
        },
        serde_json::Value::Bool(b) => {
            state.write_u8(1);
            b.hash(state);
        },
        serde_json::Value::Number(n) => {
            state.write_u8(2);
            n.hash(state);
        },
        serde_json::Value::String(s) => {
            state.write_u8(3);
            s.hash(state);
        },
        serde_json::Value::Array(a) => {
            state.write_u8(4);
            for v in a {
                hash_jsvalue(v, state);
            }
        },
        serde_json::Value::Object(o) => {
            state.write_u8(5);
            let mut keys: Vec<_> = o.keys().collect();
            keys.sort();
            for k in keys {
                k.hash(state);
                hash_jsvalue(&o[k], state);
            }
        },
    }
}
impl Hash for HttpResponseOptions {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for (k, v) in &self.headers {
            k.hash(state);
            hash_jsvalue(v, state);
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Destroyed,
    Destroying,
    Started,
    Stopped,
    Created,
}
impl State {
    pub fn from_name(name: &str) -> Option<Self> {
        // Gross hack, abuse serde to do the work for us
        serde_json::from_str(&format!("\"{}\"", name)).ok()
    }
    pub fn name(&self) -> String {
        // Gross hack, abuse serde to do the work for us
        serde_json::to_string(self).unwrap().trim_matches('"').to_string()
    }
}
impl Display for State {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name())
    }
}

use crate::entities;

pub trait AsMachineId: Sized {
    fn as_machine_id(&self) -> &str;
}
impl<T: AsRef<str>> AsMachineId for T {
    fn as_machine_id(&self) -> &str {
        self.as_ref()
    }
}
impl AsMachineId for entities::machine::Machine {
    fn as_machine_id(&self) -> &str {
        &self.id
    }
}

#[derive(Debug, serde::Serialize, Clone, Default)]
pub struct LaunchMachineInput {
    pub config: Option<entities::machine::Config>,
    pub region: Option<String>,
    pub name: Option<String>,
    pub skip_launch: bool,
    pub lease_ttl: Option<i32>,

    // Client side only
    #[serde(skip)]
    pub id: Option<String>,
}

#[derive(Debug, serde::Deserialize, Clone)]
pub struct MachineStartResponse {
    pub message: String,
    pub status: String,
    pub previous_state: String,
}


#[derive(Debug, serde::Serialize, Clone)]
pub struct StopMachineInput {
    pub id: String,
    pub signal: String,
    pub timeout: entities::FlyctlDuration,
}

#[derive(Debug, serde::Serialize, Clone)]
pub struct RestartMachineInput {
    pub id: String,
    pub signal: Option<String>,
    pub timeout: Option<std::time::Duration>,
    pub force_stop: bool,
    // pub skip_health_checks: bool,
}

#[derive(Debug, Clone)]
pub struct FlyAppsMachines {
    pub machines: Vec<entities::machine::Machine>,
    pub release_cmd_machine: Option<entities::machine::Machine>,
}

#[derive(Debug, Clone)]
pub struct RemoveMachineInput {
    pub id: String,
    pub kill: bool,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Signal {
    pub signal: i32,
}

impl From<i32> for Signal {
    fn from(signal: i32) -> Self {
        Self { signal }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct MachineLease {
    pub status: String,
    pub data: MachineLeaseData,
    pub message: String,
    pub code: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct MachineLeaseData {
    pub nonce: String,
    pub expires_at: i64,
    pub owner: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MachineExecRequest {
    pub cmd: String,
    pub timeout: i32,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct MachineExecResponse {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}
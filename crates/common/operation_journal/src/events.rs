use serde::{Deserialize, Serialize};

pub type RequestId = String;
pub type Timestamp = u64;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Event {
    pub time: Timestamp,
    pub id: RequestId,
    pub event: OperationEvent,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub enum OperationEvent {
    Schedule(OperationRequest),
    Executing(OperationProcess),
    Success(OperationStatus),
    Failure(OperationStatus)
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OperationRequest {
    pub operation: String,
    pub command: String,
    pub args: Vec<String>,
    pub user: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OperationProcess {
    pub operation: String,
    pub process_id: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OperationStatus {
    pub operation: String,
    pub log: String,
}

impl Event {
    pub fn schedule(time: u64, id: &str, operation: &str, command: &str) -> Event {
        let id = id.to_string();
        let request = OperationRequest {
            operation: operation.into(),
            command: command.into(),
            args: vec![],
            user: "nobody".into(),
        };
        Event {
            time, id, event: OperationEvent::Schedule(request)
        }
    }
}
use serde_json::{json, Value};
use std::{collections::VecDeque, sync::Mutex};
#[derive(Default)]
pub struct Events(Mutex<VecDeque<Value>>);
impl Events {
    pub fn push(&self, topic: &str, payload: Value) {
        if let Ok(mut queue) = self.0.lock() {
            if topic == "releasehub:download" {
                let id = payload.get("id");
                queue
                    .retain(|event| !(event["topic"] == topic && event["payload"].get("id") == id));
            } else if queue.iter().any(|event| event["topic"] == topic) {
                return;
            }
            if queue.len() >= 128 {
                queue.pop_front();
            }
            queue.push_back(json!({"topic":topic,"payload":payload}));
        }
    }
    pub fn drain(&self) -> Value {
        json!({"events": self.0.lock().map(|mut queue| queue.drain(..).collect::<Vec<_>>()).unwrap_or_default()})
    }
}

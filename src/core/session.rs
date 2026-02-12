/// Session management for model_call_id reuse across HTTP requests
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use once_cell::sync::Lazy;

/// Session ID type (UUID string)
pub type SessionId = String;

/// Model Call ID type
pub type ModelCallId = String;

/// Global session store
static SESSION_STORE: Lazy<Arc<SessionStore>> = Lazy::new(|| Arc::new(SessionStore::new()));

/// Session store that maps session_id to model_call_id
pub struct SessionStore {
    /// Map: session_id -> model_call_id
    sessions: RwLock<HashMap<SessionId, ModelCallId>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Get global instance
    pub fn global() -> Arc<SessionStore> {
        SESSION_STORE.clone()
    }

    /// Get or create model_call_id for a session
    pub async fn get_or_create(&self, session_id: &str) -> Option<ModelCallId> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned()
    }

    /// Save model_call_id for a session
    pub async fn save(&self, session_id: String, model_call_id: String) {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id, model_call_id);
    }

    /// Remove a session
    pub async fn remove(&self, session_id: &str) {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
    }

    /// Clear all sessions (for testing)
    #[allow(dead_code)]
    pub async fn clear(&self) {
        let mut sessions = self.sessions.write().await;
        sessions.clear();
    }

    /// Get session count (for debugging)
    #[allow(dead_code)]
    pub async fn len(&self) -> usize {
        let sessions = self.sessions.read().await;
        sessions.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_store() {
        let store = SessionStore::new();
        
        // Initial state
        assert_eq!(store.get_or_create("session1").await, None);
        
        // Save
        store.save("session1".to_string(), "model_call_id_1".to_string()).await;
        assert_eq!(store.get_or_create("session1").await, Some("model_call_id_1".to_string()));
        
        // Remove
        store.remove("session1").await;
        assert_eq!(store.get_or_create("session1").await, None);
    }
}

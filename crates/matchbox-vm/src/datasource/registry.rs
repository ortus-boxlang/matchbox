use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use super::traits::DbDriver;

// A process-wide registry so pools are never dropped on a test thread — dropping
// an r2d2_postgres pool triggers tokio-postgres cleanup which requires the Tokio
// thread-local context to still be alive.  A global static outlives any
// individual thread, avoiding the "Tokio context thread-local variable has been
// destroyed" panic during thread teardown.
static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<dyn DbDriver>>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, Arc<dyn DbDriver>>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn register(name: &str, driver: Arc<dyn DbDriver>) {
    registry().lock().unwrap().insert(name.to_string(), driver);
}

pub fn get(name: &str) -> Option<Arc<dyn DbDriver>> {
    registry().lock().unwrap().get(name).cloned()
}

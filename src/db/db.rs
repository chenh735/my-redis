use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{ RwLock};

pub fn new_db() ->Arc<RwLock<HashMap<String, String>>>{
    Arc::new(RwLock::new(HashMap::<String, String>::new()))
}
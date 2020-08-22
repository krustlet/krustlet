use crate::pod::Handle;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::BTreeMap;

type Name = String;

type Namespace = String;

#[derive(Clone)]
struct PodManager<H,F> {
    handles: Arc<RwLock<BTreeMap<(Namespace, Name),Handle<H,F>>>>
}

impl <H,F> PodManager<H,F> {
     pub fn new() -> Self {
         PodManager {
             handles: Arc::new(RwLock::new(BTreeMap::new()))
         }
     }
}

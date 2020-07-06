//! `composite` implements building complex stores from simpler ones.

use crate::store::PullPolicy;
use crate::store::Store;
use async_trait::async_trait;
use oci_distribution::Reference;
use std::sync::Arc;

/// TODO
pub trait InterceptingStore: Store {
    /// TODO
    fn intercepts(&self, image_ref: &Reference) -> bool;
}

/// TODO
pub trait ComposableStore {
    /// TODO
    fn with_override(
        self,
        interceptor: Arc<dyn InterceptingStore + Send + Sync>,
    ) -> Arc<dyn Store + Send + Sync>;
}

impl ComposableStore for Arc<dyn Store + Send + Sync> {
    fn with_override(
        self,
        interceptor: Arc<dyn InterceptingStore + Send + Sync>,
    ) -> Arc<dyn Store + Send + Sync> {
        Arc::new(CompositeStore {
            base: self,
            interceptor,
        })
    }
}

impl<S> ComposableStore for Arc<S>
where
    S: Store + Send + Sync + 'static,
{
    fn with_override(
        self,
        interceptor: Arc<dyn InterceptingStore + Send + Sync>,
    ) -> Arc<dyn Store + Send + Sync> {
        Arc::new(CompositeStore {
            base: self,
            interceptor,
        })
    }
}

/// TODO
pub struct CompositeStore {
    base: Arc<dyn Store + Send + Sync>,
    interceptor: Arc<dyn InterceptingStore + Send + Sync>,
}

#[async_trait]
impl Store for CompositeStore {
    async fn get(
        &self,
        image_ref: &Reference,
        pull_policy: Option<PullPolicy>,
    ) -> anyhow::Result<Vec<u8>> {
        if self.interceptor.intercepts(image_ref) {
            self.interceptor.get(image_ref, pull_policy).await
        } else {
            self.base.get(image_ref, pull_policy).await
        }
    }
}

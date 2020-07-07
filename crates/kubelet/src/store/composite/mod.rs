//! `composite` implements building complex stores from simpler ones.

use crate::store::PullPolicy;
use crate::store::Store;
use async_trait::async_trait;
use oci_distribution::Reference;
use std::sync::Arc;

/// A `Store` that has additional logic to determine if it can satisfy
/// a particular reference. An `InterceptingStore` can be composed with
/// another Store to satisfy specific requests in a custom way.
pub trait InterceptingStore: Store {
    /// Whether this `InterceptingStore` can satisfy the given reference.
    fn intercepts(&self, image_ref: &Reference) -> bool;
}

/// Provides a way to overlay an `InterceptingStore` so that the
/// interceptor handles the references it can, and the base store
/// handles all other references.
pub trait ComposableStore {
    /// Creates a `Store` identical to the implementer except that
    /// 'get' requests are offered to the interceptor first.
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

struct CompositeStore {
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

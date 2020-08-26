//! `composite` implements building complex stores from simpler ones.

use crate::store::PullPolicy;
use crate::store::Store;
use async_trait::async_trait;
use oci_distribution::secrets::RegistryAuth;
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
        pull_policy: PullPolicy,
        auth: &RegistryAuth,
    ) -> anyhow::Result<Vec<u8>> {
        if self.interceptor.intercepts(image_ref) {
            self.interceptor.get(image_ref, pull_policy, auth).await
        } else {
            self.base.get(image_ref, pull_policy, auth).await
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use oci_distribution::secrets::RegistryAuth;
    use std::convert::TryFrom;

    struct FakeBase {}
    struct FakeInterceptor {}

    #[async_trait]
    impl Store for FakeBase {
        async fn get(
            &self,
            _image_ref: &Reference,
            _pull_policy: PullPolicy,
            _auth: &RegistryAuth,
        ) -> anyhow::Result<Vec<u8>> {
            Ok(vec![11, 10, 5, 14])
        }
    }

    #[async_trait]
    impl Store for FakeInterceptor {
        async fn get(
            &self,
            _image_ref: &Reference,
            _pull_policy: PullPolicy,
            _auth: &RegistryAuth,
        ) -> anyhow::Result<Vec<u8>> {
            Ok(vec![1, 2, 3])
        }
    }

    impl InterceptingStore for FakeInterceptor {
        fn intercepts(&self, image_ref: &Reference) -> bool {
            image_ref.whole().starts_with("int")
        }
    }

    #[tokio::test]
    async fn if_interceptor_matches_then_composite_store_returns_intercepting_value() {
        let store = Arc::new(FakeBase {}).with_override(Arc::new(FakeInterceptor {}));
        let result = store
            .get(
                &Reference::try_from("int/foo").unwrap(),
                PullPolicy::Never,
                &RegistryAuth::Anonymous,
            )
            .await
            .unwrap();
        assert_eq!(3, result.len());
        assert_eq!(1, result[0]);
    }

    #[tokio::test]
    async fn if_interceptor_does_not_match_then_composite_store_returns_base_value() {
        let store = Arc::new(FakeBase {}).with_override(Arc::new(FakeInterceptor {}));
        let result = store
            .get(
                &Reference::try_from("mint/foo").unwrap(),
                PullPolicy::Never,
                &RegistryAuth::Anonymous,
            )
            .await
            .unwrap();
        assert_eq!(4, result.len());
        assert_eq!(11, result[0]);
    }
}

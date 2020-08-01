//! Macro for defining state graphs.

#[macro_export]
/// Easily define state machine states and behavior.
macro_rules! state {
    (
       $(#[$meta:meta])*
       $name:ident,
       $state:path,
       $success:ty,
       $error: ty,
       $work:block,
       $patch:block
    ) => {
        $(#[$meta])*
        #[derive(Default, Debug)]
        pub struct $name;


        #[async_trait::async_trait]
        impl <P: $state> State<P> for $name {
            type Success = $success;
            type Error = $error;

            async fn next(
                self,
                #[allow(unused_variables)] provider: Arc<P>,
                #[allow(unused_variables)] pod: &Pod,
            ) -> anyhow::Result<Transition<Self::Success, Self::Error>> {
                #[allow(unused_braces)]
                $work
            }

            async fn json_status(
                &self,
                #[allow(unused_variables)] provider: Arc<P>,
                #[allow(unused_variables)] pod: &Pod,
            ) -> anyhow::Result<serde_json::Value> {
                #[allow(unused_braces)]
                $patch
            }
        }
    };
    (
       $(#[$meta:meta])*
       $name:ident,
       $state:path,
       $success:ty,
       $error: ty,
       $work:path,
       $patch:block
    ) => {
        $(#[$meta])*
        #[derive(Default, Debug)]
        pub struct $name;


        #[async_trait::async_trait]
        impl <P: $state> State<P> for $name {
            type Success = $success;
            type Error = $error;

            async fn next(
                self,
                #[allow(unused_variables)] provider: Arc<P>,
                #[allow(unused_variables)] pod: &Pod,
            ) -> anyhow::Result<Transition<Self::Success, Self::Error>> {
                $work(self, pod).await
            }

            async fn json_status(
                &self,
                #[allow(unused_variables)] provider: Arc<P>,
                #[allow(unused_variables)] pod: &Pod,
            ) -> anyhow::Result<serde_json::Value> {
                #[allow(unused_braces)]
                $patch
            }
        }
    };



}

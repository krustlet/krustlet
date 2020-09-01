//! Macro for defining state graphs.

#[macro_export]
/// Easily define state machine states and behavior.
macro_rules! state {
    (
       $(#[$meta:meta])*
       $name:ident,
       $state:ty,
       $work:block,
       $patch:block
    ) => {
        $(#[$meta])*
        #[derive(Default, Debug)]
        pub struct $name;


        #[async_trait::async_trait]
        impl State<$state> for $name {
            async fn next(
                &self,
                #[allow(unused_variables)] pod_state: &mut $state,
                #[allow(unused_variables)] pod: &Pod,
            ) -> anyhow::Result<Transition<Box<dyn State<$state>>,Box<dyn State<$state>>>> {
                #[allow(unused_braces)]
                $work
            }

            async fn json_status(
                &self,
                #[allow(unused_variables)] pod_state: &mut $state,
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
       $state:ty,
       $work:path,
       $patch:block
    ) => {
        $(#[$meta])*
        #[derive(Default, Debug)]
        pub struct $name;


        #[async_trait::async_trait]
        impl State<$state> for $name {
            async fn next(
                &self,
                #[allow(unused_variables)] pod_state: &mut $state,
                #[allow(unused_variables)] pod: &Pod,
            ) -> anyhow::Result<Transition<Box<dyn State<$state>>,Box<dyn State<$state>>>> {
                $work(self, pod).await
            }

            async fn json_status(
                &self,
                #[allow(unused_variables)] pod_state: &mut $state,
                #[allow(unused_variables)] pod: &Pod,
            ) -> anyhow::Result<serde_json::Value> {
                #[allow(unused_braces)]
                $patch
            }
        }
    };
}

//! Defines types for registering controllers with runtime.
use crate::{operator::Operator, store::Store};

pub mod tasks;
use tasks::{controller_tasks, OperatorTask};

pub mod controller;
use controller::{Controller, ControllerBuilder};
mod watch;

/// Coordinates one or more controllers and the main entrypoint for starting
/// the application.
///
/// # Warning
///
/// This API does not support admissions webhooks yet, please
/// use [OperatorRuntime](crate::runtime::OperatorRuntime).
pub struct Manager {
    kubeconfig: kube::Config,
    controllers: Vec<Controller>,
    controller_tasks: Vec<OperatorTask>,
    store: Store,
}

impl Manager {
    /// Create a new controller manager.
    pub fn new(kubeconfig: &kube::Config) -> Self {
        Manager {
            controllers: vec![],
            controller_tasks: vec![],
            kubeconfig: kubeconfig.clone(),
            store: Store::new(),
        }
    }

    /// Register a controller with the manager.
    pub fn register_controller<C: Operator>(&mut self, builder: ControllerBuilder<C>) {
        let (controller, tasks) =
            controller_tasks(self.kubeconfig.clone(), builder, self.store.clone());
        self.controllers.push(controller);
        self.controller_tasks.extend(tasks);
    }

    /// Start the manager, blocking forever.
    pub async fn start(self) {
        use futures::FutureExt;
        use std::convert::TryFrom;
        use tasks::launch_watcher;

        let mut tasks = self.controller_tasks;
        let client = kube::Client::try_from(self.kubeconfig)
            .expect("Unable to create kube::Client from kubeconfig.");

        // TODO: Deduplicate Watchers
        for controller in self.controllers {
            tasks.push(launch_watcher(client.clone(), controller.manages).boxed());
            for handle in controller.owns {
                tasks.push(launch_watcher(client.clone(), handle).boxed());
            }
            for handle in controller.watches {
                tasks.push(launch_watcher(client.clone(), handle).boxed());
            }
        }

        futures::future::join_all(tasks).await;
    }
}

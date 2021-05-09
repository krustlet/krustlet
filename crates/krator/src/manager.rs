//! Defines types for registring controllers with runtime.

use std::sync::Arc;

use futures::FutureExt;

use crate::{
    operator::Operator,
    store::Store,
};

pub mod tasks;
use tasks::{
    controller_tasks,
    OperatorTask
};

mod controller;
use controller::Controller;
pub use controller::ControllerBuilder;
mod watch;
use watch::launch_watcher;


/// Coordinates one or more controllers and the main entrypoint for starting
/// the application.
// #[derive(Default)]
pub struct Manager {
    kubeconfig: kube::Config,
    controllers: Vec<Controller>,
    controller_tasks: Vec<OperatorTask>,
    store: Arc<Store>,
}

impl Manager {
    /// Create a new controller manager.
    pub fn new(kubeconfig: &kube::Config) -> Self {
        Manager {
            controllers: vec![],
            controller_tasks: vec![],
            kubeconfig: kubeconfig.clone(),
            store: Arc::new(Store::new()),
        }
    }

    /// Register a controller with the manager.
    pub fn register_controller<C: Operator>(&mut self, builder: ControllerBuilder<C>) {
        let (controller, tasks) =
            controller_tasks(self.kubeconfig.clone(), builder, Arc::clone(&self.store));
        self.controllers.push(controller);
        self.controller_tasks.extend(tasks);
    }

    /// Start the manager, blocking forever.
    pub async fn start(self) {
        use std::convert::TryFrom;

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

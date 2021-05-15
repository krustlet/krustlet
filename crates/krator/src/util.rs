//! Provides some utility functions for Krator.

use kube::{api::DynamicObject, Resource};
use kube_runtime::watcher::Event;
use serde::de::DeserializeOwned;

#[derive(Debug)]
/// Utility struct for summarizing `kube_runtime::watcher::Event` in log
/// output.
///
/// ```
/// # use kube_runtime::watcher::Event;
/// # use k8s_openapi::api::core::v1::Pod;
/// # use tracing::info;
/// # let event: Event<Pod> = Event::Restarted(vec![]);
/// use krator::util::PrettyEvent;
/// info!(event=?PrettyEvent::from(&event));
/// ```
pub enum PrettyEvent {
    /// Represents `Event::Applied`. A single object was updated.
    Applied {
        /// Name of the object.
        name: String,
        /// Namespace of the object if applicable.
        namespace: Option<String>,
    },
    /// Represents `Event::Deleted`. A single object was deleted.
    Deleted {
        /// Name of the object.
        name: String,
        /// Namespace of the object if applicable.
        namespace: Option<String>,
    },
    /// Represents `Event::Restart`. Full refresh with current state of all
    /// objects.
    Restarted {
        /// Number of objects in refresh.
        count: usize,
    },
}

impl<R: Resource> From<&Event<R>> for PrettyEvent {
    fn from(event: &Event<R>) -> Self {
        match event {
            Event::Applied(object) => PrettyEvent::Applied {
                name: object.name(),
                namespace: object.namespace(),
            },
            Event::Deleted(object) => PrettyEvent::Deleted {
                name: object.name(),
                namespace: object.namespace(),
            },
            Event::Restarted(objects) => PrettyEvent::Restarted {
                count: objects.len(),
            },
        }
    }
}

/// Convert `kube::api::DynamicObject` to a concrete type which must implement
/// `DeserializeOwned`.
///
/// For now this simply serializes the `DynamicObject` to JSON and then
/// deserializes to the desired type.
///
/// # Errors
///
/// If serialization of deserialization fail.
///
/// # Examples
///
/// ```
/// # use k8s_openapi::api::core::v1::Pod;
/// # use kube::api::DynamicObject;
/// # use kube::api::GroupVersionKind;
/// use krator::util::concrete_object;
/// # let pod = Pod::default();
/// # let dynamic_object = serde_json::from_str::<DynamicObject>(&serde_json::to_string(&pod).unwrap()).unwrap();
/// let pod = concrete_object::<Pod>(dynamic_object).unwrap();
/// ```
pub fn concrete_object<R>(dynamic_object: DynamicObject) -> anyhow::Result<R>
where
    R: DeserializeOwned,
{
    // TODO: This sucks
    let value = serde_json::to_value(&dynamic_object)?;
    Ok(serde_json::from_value::<R>(value)?)
}

/// Convert [DynamicEvent](crate::util::DynamicEvent) to
/// concrete event `kube_runtime::watcher::Event<R>` where `R` implements
/// `DeserializeOwned`.
///
/// Relies on [concrete_object](crate::util::concrete_object). See for error
/// conditions.
///
/// # Examples
/// ```
/// # use k8s_openapi::api::core::v1::Pod;
/// # use kube_runtime::watcher::Event;
/// # use krator::util::{DynamicEvent, concrete_event};
/// let dynamic_event: DynamicEvent = Event::Restarted(vec![]);
/// let event = concrete_event::<Pod>(dynamic_event).unwrap();
///
/// ```
pub fn concrete_event<R>(dynamic_event: DynamicEvent) -> anyhow::Result<Event<R>>
where
    R: DeserializeOwned,
{
    match dynamic_event {
        Event::Applied(dynamic_object) => Ok(Event::Applied(concrete_object(dynamic_object)?)),
        Event::Deleted(dynamic_object) => Ok(Event::Deleted(concrete_object(dynamic_object)?)),
        Event::Restarted(dynamic_objects) => Ok(Event::Restarted(
            dynamic_objects
                .into_iter()
                .map(concrete_object)
                .collect::<anyhow::Result<Vec<R>>>()?,
        )),
    }
}

/// Used to refer to `kube_runtime::watcher::Event<kube::api::DynamicObject>`.
pub type DynamicEvent = Event<DynamicObject>;

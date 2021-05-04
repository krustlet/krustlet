//! Defines types for storing type-erased objects and events.
use kube_runtime::watcher::Event;

pub struct DynamicObject {
    pub name: String,
    pub namespace: Option<String>,
    pub data: Box<dyn std::any::Any + Sync + Send>,
}

impl std::fmt::Debug for DynamicObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DynamicObject")
            .field("name", &self.name)
            .field("namespace", &self.namespace)
            .finish()
    }
}

#[derive(Debug)]
pub enum DynamicEvent {
    Applied {
        object: DynamicObject,
    },
    Deleted {
        name: String,
        namespace: Option<String>,
        object: DynamicObject,
    },
    Restarted {
        objects: Vec<DynamicObject>,
    },
}

impl<R: kube::Resource + 'static + Sync + Send> From<R> for DynamicObject {
    fn from(object: R) -> Self {
        DynamicObject {
            name: object.name(),
            namespace: object.namespace(),
            data: Box::new(object),
        }
    }
}

impl<R: kube::Resource + 'static + Sync + Send> std::convert::TryFrom<DynamicEvent> for Event<R> {
    type Error = anyhow::Error;

    fn try_from(event: DynamicEvent) -> anyhow::Result<Self> {
        match event {
            DynamicEvent::Applied { object } => match object.data.downcast::<R>() {
                Ok(object) => Ok(Event::Applied(*object)),
                Err(e) => anyhow::bail!("Could now downcast dynamic type: {:?}", e),
            },
            DynamicEvent::Deleted { object, .. } => match object.data.downcast::<R>() {
                Ok(object) => Ok(Event::Applied(*object)),
                Err(e) => anyhow::bail!("Could now downcast dynamic type: {:?}", e),
            },
            DynamicEvent::Restarted {
                objects: dynamic_objects,
            } => {
                let mut objects: Vec<R> = Vec::new();
                for object in dynamic_objects {
                    match object.data.downcast::<R>() {
                        Ok(object) => objects.push(*object),
                        Err(e) => anyhow::bail!("Could now downcast dynamic type: {:?}", e),
                    }
                }
                Ok(Event::Restarted(objects))
            }
        }
    }
}

impl<R: kube::Resource + 'static + Sync + Send> From<Event<R>> for DynamicEvent {
    fn from(event: Event<R>) -> Self {
        match event {
            Event::Applied(object) => DynamicEvent::Applied {
                object: object.into(),
            },
            Event::Deleted(object) => DynamicEvent::Deleted {
                name: object.name(),
                namespace: object.namespace(),
                object: object.into(),
            },
            Event::Restarted(objects) => {
                let mut dynamic_objects = Vec::with_capacity(objects.len());
                for object in objects {
                    dynamic_objects.push(object.into());
                }
                DynamicEvent::Restarted {
                    objects: dynamic_objects,
                }
            }
        }
    }
}

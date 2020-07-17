// This library was adapted from the original wascc logging
// implementation contributed by Brian Ketelsen to wascc.
// Original license below:

// Copyright 2015-2019 Capital One Services, LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#[cfg(not(feature = "static_plugin"))]
#[macro_use]
extern crate wascc_codec;

use wascc_codec::capabilities::{
    CapabilityDescriptor, CapabilityProvider, Dispatcher, NullDispatcher, OperationDirection,
    OP_GET_CAPABILITY_DESCRIPTOR,
};
use wascc_codec::core::{CapabilityConfiguration, OP_BIND_ACTOR, OP_REMOVE_ACTOR};
use wascc_codec::{
    deserialize,
    logging::{WriteLogRequest, OP_LOG},
    serialize,
};

extern crate log;
use log::Log;

use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::error::Error;
use std::fs::{File, OpenOptions};
use std::sync::RwLock;

use simplelog::{Config, LevelFilter, WriteLogger};

#[cfg(not(feature = "static_plugin"))]
capability_provider!(LoggingProvider, LoggingProvider::new);

pub const LOG_PATH_KEY: &str = "LOG_PATH";

/// Origin of messages coming from wascc host
const SYSTEM_ACTOR: &str = "system";

const CAPABILITY_ID: &str = "wascc:logging";

const VERSION: &str = env!("CARGO_PKG_VERSION");

enum LogLevel {
    NONE = 0,
    ERROR,
    WARN,
    INFO,
    DEBUG,
    TRACE,
}

/// LoggingProvider provides an implementation of the wascc:logging capability
/// that keeps separate log output for each actor.
pub struct LoggingProvider {
    dispatcher: RwLock<Box<dyn Dispatcher>>,
    output_map: RwLock<HashMap<String, Box<WriteLogger<File>>>>,
}

impl Default for LoggingProvider {
    fn default() -> Self {
        LoggingProvider {
            dispatcher: RwLock::new(Box::new(NullDispatcher::new())),
            output_map: RwLock::new(HashMap::new()),
        }
    }
}

impl LoggingProvider {
    pub fn new() -> Self {
        Self::default()
    }

    fn configure(&self, config: CapabilityConfiguration) -> Result<Vec<u8>, Box<dyn Error>> {
        let path = config
            .values
            .get(LOG_PATH_KEY)
            .ok_or("log file path was unspecified")?;

        let file = OpenOptions::new().write(true).open(path)?;
        let logger = WriteLogger::new(LevelFilter::Trace, Config::default(), file);
        let mut output_map = self.output_map.write().unwrap();
        output_map.insert(config.module, logger);
        Ok(vec![])
    }

    fn get_descriptor(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        Ok(serialize(
            CapabilityDescriptor::builder()
                .id(CAPABILITY_ID)
                .name("krustlet Logging Provider")
                .long_description("A waSCC logging capability provider")
                .version(VERSION)
                // NOTE(bacongobbler): this crate is never published, so we never need to increment the revision above 1
                .revision(1)
                .with_operation(OP_LOG, OperationDirection::ToProvider, "Send a log message")
                .build(),
        )?)
    }
}

impl CapabilityProvider for LoggingProvider {
    // Invoked by the runtime host to give this provider plugin the ability to communicate
    // with actors
    fn configure_dispatch(&self, dispatcher: Box<dyn Dispatcher>) -> Result<(), Box<dyn Error>> {
        let mut lock = self.dispatcher.write().unwrap();
        *lock = dispatcher;

        Ok(())
    }

    // Invoked by host runtime to allow an actor to make use of the capability
    // All providers MUST handle the "configure" message, even if no work will be done
    fn handle_call(&self, actor: &str, op: &str, msg: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
        match (op, actor) {
            (OP_BIND_ACTOR, SYSTEM_ACTOR) => {
                let cfg_vals = deserialize::<CapabilityConfiguration>(msg)?;
                self.configure(cfg_vals)
            }
            (OP_REMOVE_ACTOR, SYSTEM_ACTOR) => Ok(vec![]),
            (OP_GET_CAPABILITY_DESCRIPTOR, SYSTEM_ACTOR) => self.get_descriptor(),
            (OP_LOG, _) => {
                let log_msg = deserialize::<WriteLogRequest>(msg)?;

                let level = match log_msg.level.try_into() {
                    Ok(LogLevel::ERROR) => log::Level::Error,
                    Ok(LogLevel::WARN) => log::Level::Warn,
                    Ok(LogLevel::INFO) => log::Level::Info,
                    Ok(LogLevel::DEBUG) => log::Level::Debug,
                    Ok(LogLevel::TRACE) => log::Level::Trace,
                    Ok(LogLevel::NONE) => return Ok(vec![]),
                    _ => return Err(format!("Unknown log level {}", log_msg.level).into()),
                };

                let output_map = self.output_map.read().unwrap();
                let logger = output_map
                    .get(actor)
                    .ok_or(format!("Unable to find logger for actor {}", actor))?;
                logger.log(
                    &log::Record::builder()
                        .args(format_args!("[{}] {}", actor, log_msg.body))
                        .level(level)
                        .build(),
                );
                Ok(vec![])
            }
            _ => Err(format!("Unknown operation: {}", op).into()),
        }
    }
}

impl TryFrom<u32> for LogLevel {
    type Error = ();
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        let level = match value {
            x if x == LogLevel::ERROR as u32 => LogLevel::ERROR,
            x if x == LogLevel::WARN as u32 => LogLevel::WARN,
            x if x == LogLevel::INFO as u32 => LogLevel::INFO,
            x if x == LogLevel::DEBUG as u32 => LogLevel::DEBUG,
            x if x == LogLevel::TRACE as u32 => LogLevel::TRACE,
            x if x == LogLevel::NONE as u32 => LogLevel::NONE,
            _ => return Err(()),
        };
        Ok(level)
    }
}

pub mod dependencies;
#[allow(dead_code)]
mod java;
#[allow(dead_code)]
mod model;
pub mod parse;
#[allow(dead_code)]
mod spring;
pub mod transport;
mod ui;
pub mod yapi;
pub use ui::YapiPlugin;
openidea_plugin::register_plugin!(YapiPlugin, views);

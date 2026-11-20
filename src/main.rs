use std::path::Path;

use anyhow::Result;

use crate::config::Config;
use crate::endpoints::Endpoints;

pub mod config;
pub mod daft_ie;
pub mod dedupe;
pub mod delay;
pub mod endpoints;
pub mod http;
pub mod listing;
pub mod next_data;
pub mod pipeline;
pub mod places;
pub mod property_ie;
pub mod rent_ie;
pub mod routing;
pub mod slack;
pub mod state;
#[cfg(test)]
mod test_support;
pub mod text;

fn main() -> Result<()> {
    // State files are read and written relative to the working directory.
    pipeline::run(Config::from_env()?, &Endpoints::default(), Path::new("."))
}

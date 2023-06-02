use std::collections::HashMap;

use lopdf::Dictionary;

pub trait Filter {
    fn filter(&self, data: Vec<u8>, params: &Dictionary) -> Result<Vec<u8>, anyhow::Error>;
}

#[derive(educe::Educe)]
#[educe(Default(new))]
pub struct FilterFactory {
    filters: HashMap<&'static str, Box<dyn Filter>>,
}

/// Error returned by [`FilterFactory::create()`].
#[derive(thiserror::Error, Debug)]
pub enum FilterFactoryError {
    #[error("Unknown filter {0}")]
    UnknownFilter(String),
}

impl FilterFactory {
    pub fn create(&self, name: &str) -> Result<Box<dyn Filter>, FilterFactoryError> {
        match name {
            // "FlateDecode" => Ok(Box::new(FlateDecode {})),
            _ => Err(FilterFactoryError::UnknownFilter(name.to_owned())),
        }
    }
}

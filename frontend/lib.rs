#![allow(unused_features)]
pub mod application;
pub mod entities;
pub mod features;
pub mod shared;
pub mod views;

pub use application::router::{App, Route};

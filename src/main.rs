mod common;
mod configs;
mod domain;
mod event_sourcing;
mod repository;
mod service;

fn main() {
    dotenv::dotenv().ok();
    println!("Hello, world!");
}

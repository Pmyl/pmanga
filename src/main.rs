mod bridge;
mod components;
mod input;
mod pages;
mod routes;
mod storage;

use routes::App;

fn main() {
    dioxus::launch(App);
}

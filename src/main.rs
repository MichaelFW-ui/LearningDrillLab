mod ai;
mod app;
mod domain;
mod ui;

fn main() {
    use dioxus::desktop::{Config, WindowBuilder};

    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::new().with_window(
                WindowBuilder::new()
                    .with_title("Learning Drill Lab")
                    .with_always_on_top(false),
            ),
        )
        .launch(app::App);
}

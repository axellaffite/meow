mod app;
mod ui;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

fn main() {
    // Claude starts the binary as its MCP server for Meow's data.
    match std::env::args().nth(1).as_deref() {
        Some(meow::context::MCP_FLAG) => return meow::context::serve(),
        // Claude Code runs it before each tool call of an autopilot run.
        Some(meow::guard::GUARD_FLAG) => return meow::guard::serve(),
        _ => {}
    }
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("meow must start on the main thread");
        std::process::exit(1);
    };
    let app = NSApplication::sharedApplication(mtm);
    // No Dock icon: the cat is the interface.
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app::start(mtm);
    app.run();
}

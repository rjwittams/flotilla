use std::io::stdout;

use crossterm::{execute, event::DisableMouseCapture};

/// Restore the terminal to its original state.
///
/// Safe to call multiple times or when mouse capture was never enabled —
/// `DisableMouseCapture` and `ratatui::restore()` are both no-ops in those cases.
pub fn restore_terminal() {
    let _ = execute!(stdout(), DisableMouseCapture);
    let _ = ratatui::restore();
}

/// Install a panic hook that restores the terminal before printing the panic.
///
/// Must be called after `ratatui::init()`. Wraps whatever hook is currently
/// installed (including color_eyre's) so error reporting still works.
pub fn install_panic_hook() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        hook(info);
    }));
}

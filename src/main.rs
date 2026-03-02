mod app;
mod event;
mod ui;

use std::time::Duration;
use color_eyre::Result;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let mut terminal = ratatui::init();
    let result = run(&mut terminal).await;
    ratatui::restore();
    result
}

async fn run(terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
    let mut app = app::App::default();
    let mut events = event::EventHandler::new(Duration::from_millis(250));

    loop {
        terminal.draw(|f| ui::render(&app, f))?;

        if let Some(evt) = events.next().await {
            match evt {
                event::Event::Key(k) => app.handle_key(k),
                event::Event::Tick => app.tick(),
            }
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}

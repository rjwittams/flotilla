use std::time::Duration;

use color_eyre::Result;
use crossterm::event::{EventStream, KeyCode, KeyEventKind};
use futures::{FutureExt, StreamExt};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style, Stylize},
    widgets::{Block, Paragraph},
    DefaultTerminal, Frame,
};

#[derive(Default)]
struct App {
    should_quit: bool,
}

impl App {
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            _ => {}
        }
    }

    fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let block = Block::bordered().title(" cmux-controller ");
        let text = Paragraph::new("Press q to quit")
            .block(block)
            .style(Style::default().fg(Color::White));
        frame.render_widget(text, area);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let mut terminal = ratatui::init();
    let result = run(&mut terminal).await;
    ratatui::restore();
    result
}

async fn run(terminal: &mut DefaultTerminal) -> Result<()> {
    let mut app = App::default();
    let mut reader = EventStream::new();
    let tick_rate = Duration::from_millis(250);
    let mut interval = tokio::time::interval(tick_rate);

    loop {
        terminal.draw(|f| app.render(f))?;

        let delay = interval.tick();
        let event = reader.next().fuse();

        tokio::select! {
            _ = delay => {}
            maybe = event => match maybe {
                Some(Ok(crossterm::event::Event::Key(k))) if k.kind == KeyEventKind::Press => {
                    app.handle_key(k);
                }
                _ => {}
            }
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}

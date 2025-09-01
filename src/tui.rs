use crate::*;

use crossterm::{
    event,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use tokio::sync::watch::Receiver;

pub enum Event {
    Quit,
}

pub fn init(is_running_rx: &Receiver<bool>, cb: impl Fn(Event)) -> Result<()> {
    // TODO: if we do like this, signals are no longer accepted
    // TODO: text shows weird like this
    enable_raw_mode().unwrap();

    loop {
        // check if still running or if already all canceled
        let is_running = *is_running_rx.borrow();
        if !is_running {
            println!("breaking the tui chain...");
            break;
        }

        let evt = event::read().unwrap().as_key_press_event();
        if evt.is_none() {
            continue;
        }

        // check for keyboard events
        let key_event = evt.unwrap();
        let signal_exit =
            event::KeyEvent::new(event::KeyCode::Char('c'), event::KeyModifiers::CONTROL);
        if key_event == signal_exit || key_event.code == event::KeyCode::Char('q') {
            cb(Event::Quit);
            break;
        }
    }

    disable_raw_mode().unwrap();

    Ok(())
}

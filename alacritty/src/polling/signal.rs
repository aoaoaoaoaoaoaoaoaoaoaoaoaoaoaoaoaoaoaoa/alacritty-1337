//! Unix signal listener.

use std::io::{Error as IoError, Read};
use std::os::unix::net::UnixStream;

use signal_hook::SigId;
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::low_level::{pipe, unregister};
use winit::event_loop::EventLoopProxy;

use crate::event::{Event, EventType};

pub struct SignalListener {
    pub pipe: UnixStream,

    event_proxy: EventLoopProxy<Event>,
    registrations: [SigId; 2],
}

impl SignalListener {
    pub fn new(event_proxy: EventLoopProxy<Event>) -> Result<Self, IoError> {
        let (pipe, write) = UnixStream::pair()?;
        let sigint = pipe::register(SIGINT, write.try_clone()?)?;
        let sigterm = match pipe::register(SIGTERM, write) {
            Ok(sigterm) => sigterm,
            Err(err) => {
                let _ = unregister(sigint);
                return Err(err);
            },
        };
        Ok(Self { pipe, event_proxy, registrations: [sigint, sigterm] })
    }

    /// Process the next signal.
    pub fn process_signal(&mut self) -> Result<(), IoError> {
        // Submit shutdown request to the main event loop.
        let event = Event::new(EventType::Shutdown, None);
        let _ = self.event_proxy.send_event(event);

        // Ensure signal is drained from pipe.
        self.pipe.read_exact(&mut [0])?;

        Ok(())
    }
}

impl Drop for SignalListener {
    fn drop(&mut self) {
        for registration in self.registrations {
            let _ = unregister(registration);
        }
    }
}

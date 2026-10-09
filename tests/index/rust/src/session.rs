/// A session that rings.
pub struct Session {
    name: String,
    rings: u32,
}

impl Session {
    pub fn new(name: &str) -> Session {
        Session {
            name: name.to_string(),
            rings: 0,
        }
    }

    /// Stops it ringing.
    pub fn stop(&mut self) {
        self.rings = 0;
    }

    pub fn rings(&self) -> u32 {
        self.rings
    }
}

/// What a session is doing.
pub enum State {
    Ringing,
    Stopped,
}

#[cfg(test)]
mod tests {
    #[test]
    fn rings() {}
}

use songbird::{ConnectionInfo, Driver};

pub struct Groover {
    driver: Driver,
    is_connected: bool,
}

impl Groover {
    pub fn new() -> Groover {
        Groover{
            driver: Driver::new(Default::default()),
            is_connected: false,
        }
    }

    pub fn connect(&mut self, info: ConnectionInfo) {
        if self.is_connected {
            self.disconnect()
        }
        self.driver.connect(info);
        self.is_connected = true;
    }

    pub fn disconnect(&mut self) {
        if self.is_connected {
            self.driver.leave()
        }
    }
}
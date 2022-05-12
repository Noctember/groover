use songbird::{Call, ConnectionInfo, Driver};
use songbird::id::{GuildId, UserId};
use songbird::input::Input;

#[derive(Clone)]
pub struct Groover {
    call: Call,
    is_connected: bool,
    pub is_source_set: bool,
}

impl Groover {
    pub fn new(guild_id: String, user_id: String) -> Groover {
        Groover {
            call : Call::standalone(GuildId::from(guild_id.clone().parse::<u64>().unwrap()), UserId::from(user_id.clone().parse::<u64>().unwrap())),
            is_connected: false,
            is_source_set: false,
        }
    }

    pub async fn connect(&mut self, info: ConnectionInfo) {
        if self.is_connected {
            self.disconnect().await;
        }
        self.call.connect(info).await;
        self.is_connected = true;
    }

    pub async fn disconnect(&mut self) {
        if self.is_connected {
            self.call.leave().await;
        }
    }

    pub fn set_source(&mut self, source: Input) {
        self.call.play_source(source);
        self.call.set_bitrate(songbird::Bitrate::Auto);
        self.is_source_set = true;
    }
}
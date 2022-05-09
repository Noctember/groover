use std::env;
use std::sync::Arc;

use futures::StreamExt;
use librespot::playback::config::Bitrate;
use songbird::{ConnectionInfo};
use songbird::id::{GuildId, UserId};
use tokio::sync::Mutex;
use serde::{Serialize, Deserialize};
use lib::player::SpotifyPlayer;

use crate::groover::Groover;

mod groover;

mod lib {
    pub mod player;
}

/*pub struct UserIdKey;

pub struct GuildIdKey;

impl TypeMapKey for UserIdKey {
    type Value = id::UserId;
}

impl TypeMapKey for GuildIdKey {
    type Value = id::GuildId;
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("Ready!");
        println!("Invite me with https://discord.com/api/oauth2/authorize?client_id={}&permissions=36700160&scope=bot", ready.user.id);

        ctx.set_presence(None, user::OnlineStatus::Online).await;
    }

    async fn cache_ready(&self, ctx: Context, guilds: Vec<id::GuildId>) {
        let guild_id = match guilds.first() {
            Some(guild_id) => *guild_id,
            None => {
                panic!("Not currently in any guilds.");
            }
        };
        let data = ctx.data.read().await;

        let player = data.get::<SpotifyPlayerKey>().unwrap().clone();
        let user_id = *data
            .get::<UserIdKey>()
            .expect("User ID placed in at initialisation.");

        // Handle case when user is in VC when bot starts
        let guild = ctx
            .cache
            .guild(guild_id)
            .await
            .expect("Could not find guild in cache.");

        let channel_id = guild
            .voice_states
            .get(&user_id)
            .and_then(|voice_state| voice_state.channel_id);
        drop(guild);

        if channel_id.is_some() {
            // Enable casting
            player.lock().await.enable_connect().await;
        }

        let c = ctx.clone();

        // Handle Spotify events
        tokio::spawn(async move {
            loop {
                let channel = player.lock().await.event_channel.clone().unwrap();
                let mut receiver = channel.lock().await;

                let event = match receiver.recv().await {
                    Some(e) => e,
                    None => {
                        // Busy waiting bad but quick and easy
                        sleep(Duration::from_millis(256)).await;
                        continue;
                    }
                };

                match event {
                    PlayerEvent::Stopped { .. } => {
                        c.set_presence(None, user::OnlineStatus::Online).await;

                        let manager = songbird::get(&c)
                            .await
                            .expect("Songbird Voice client placed in at initialisation.")
                            .clone();

                        let _ = manager.remove(guild_id).await;
                    }

                    PlayerEvent::Started { .. } => {
                        let manager = songbird::get(&c)
                            .await
                            .expect("Songbird Voice client placed in at initialisation.")
                            .clone();

                        let guild = c
                            .cache
                            .guild(guild_id)
                            .await
                            .expect("Could not find guild in cache.");

                        let channel_id = match guild
                            .voice_states
                            .get(&user_id)
                            .and_then(|voice_state| voice_state.channel_id)
                        {
                            Some(channel_id) => channel_id,
                            None => {
                                println!("Could not find user in VC.");
                                println!("{}", &user_id);
                                println!("{}", &guild_id);
                                continue;
                            }
                        };
                        let _handler = manager.join(guild_id, channel_id).await;

                        if let Some(handler_lock) = manager.get(guild_id) {
                            let mut handler = handler_lock.lock().await;

                            let mut decoder = input::codec::OpusDecoderState::new().unwrap();
                            decoder.allow_passthrough = false;

                            let source = input::Input::new(
                                true,
                                input::reader::Reader::Extension(Box::new(
                                    player.lock().await.emitted_sink.clone(),
                                )),
                                input::codec::Codec::FloatPcm,
                                input::Container::Raw,
                                None,
                            );

                            handler.set_bitrate(songbird::Bitrate::Auto);

                            handler.play_source(source);
                        } else {
                            println!("Could not fetch guild by ID.");
                        }
                    }

                    PlayerEvent::Paused { .. } => {
                        c.set_presence(None, user::OnlineStatus::Online).await;
                    }

                    PlayerEvent::Playing { track_id, .. } => {
                        let track: Result<librespot::metadata::Track, MercuryError> =
                            librespot::metadata::Metadata::get(
                                &player.lock().await.session,
                                track_id,
                            )
                                .await;

                        if let Ok(track) = track {
                            let artist: Result<librespot::metadata::Artist, MercuryError> =
                                librespot::metadata::Metadata::get(
                                    &player.lock().await.session,
                                    *track.artists.first().unwrap(),
                                )
                                    .await;

                            if let Ok(artist) = artist {
                                let listening_to = format!("{}: {}", artist.name, track.name);

                                c.set_presence(
                                    Some(gateway::Activity::listening(listening_to)),
                                    user::OnlineStatus::Online,
                                )
                                    .await;
                            }
                        }
                    }
                    PlayerEvent::VolumeSet { volume } => {
                        let data2 = c.data.read().await;
                        let player = data2.get::<SpotifyPlayerKey>().unwrap();
                        // player.lock().await.spirc.as_ref().expect("").pause()
                    }
                    _ => {}
                }
            }
        });
    }

    async fn voice_state_update(
        &self,
        ctx: Context,
        _: Option<id::GuildId>,
        old: Option<VoiceState>,
        new: VoiceState,
    ) {
        let data = ctx.data.read().await;

        let user_id = data.get::<UserIdKey>();

        if new.user_id.to_string() != user_id.unwrap().to_string() {
            return;
        }

        let player = data.get::<SpotifyPlayerKey>().unwrap();

        let guild = ctx
            .cache
            .guild(ctx.cache.guilds().await.first().unwrap())
            .await
            .unwrap();

        // If user just connected
        if old.clone().is_none() {
            // Enable casting
            player.lock().await.enable_connect().await;
            return;
        }

        // If user disconnected
        if old.clone().unwrap().channel_id.is_some() && new.channel_id.is_none() {
            // Disable casting
            player.lock().await.disable_connect().await;

            // Disconnect
            let manager = songbird::get(&ctx)
                .await
                .expect("Songbird Voice client placed in at initialisation.")
                .clone();

            let _handler = manager.remove(guild.id).await;

            return;
        }

        // If user moved channels
        if old.unwrap().channel_id.unwrap() != new.channel_id.unwrap() {
            let bot_id = ctx.cache.current_user_id().await;

            let bot_channel = guild
                .voice_states
                .get(&bot_id)
                .and_then(|voice_state| voice_state.channel_id);

            if Option::is_some(&bot_channel) {
                let manager = songbird::get(&ctx)
                    .await
                    .expect("Songbird Voice client placed in at initialisation.")
                    .clone();

                if let Some(guild_id) = ctx.cache.guilds().await.first() {
                    let _handler = manager.join(*guild_id, new.channel_id.unwrap()).await;
                }
            }

            return;
        }
    }
}*/
#[derive(Serialize, Deserialize)]
#[serde(remote = "UserId")]
struct UserIdDef(pub u64);

#[derive(Serialize, Deserialize)]
#[serde(remote = "GuildId")]
pub struct GuildIdDef(pub u64);
#[derive(Serialize, Deserialize)]
#[serde(remote = "ConnectionInfo")]
struct ConnectionInfoDef {
    endpoint: String,
    #[serde(with = "GuildIdDef")]
    guild_id: GuildId,
    session_id: String,
    token: String,
    #[serde(with = "UserIdDef")]
    user_id: UserId,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum OperatorMsg {
    Join {
        #[serde(with = "ConnectionInfoDef")]
        info: ConnectionInfo
    },
    PausePlay,

}

#[tokio::main]
async fn main() {
    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    let guild_id =
        env::var("DISCORD_GUILD_ID").expect("Expected a Discord guild ID in the environment");

    let user_id =
        env::var("DISCORD_USER_ID").expect("Expected a Discord user ID in the environment");

    let mut cache_dir = None;

    if let Ok(c) = env::var("CACHE_DIR") {
        cache_dir = Some(c);
    }

    // let player = Arc::new(Mutex::new(
    // ));
    SpotifyPlayer::new(Bitrate::Bitrate320, cache_dir).await;


    let nats_url = env::var("NATS_URL").expect("Expected a NATS URL in the environment");

    let mut  nc = async_nats::connect(nats_url).await.unwrap();

    let mut driver = Groover::new();

    let mut sub = nc.subscribe(guild_id).await.unwrap();

    tokio::spawn(async move {
        loop {
            while let Some(msg) = sub.next().await {
                let omsg: OperatorMsg = serde_json::from_slice(&msg.payload).unwrap();
                match omsg {
                    OperatorMsg::PausePlay => {
                        // player.lock().await.spirc.as_ref().unwrap().play_pause();
                    }
                    OperatorMsg::Join { info } => {
                        driver.connect(info);
                    }
                }
            }
        }
    });
}

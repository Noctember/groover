use std::{env, io};
use std::clone::Clone;
use std::env::VarError;
use std::str::FromStr;
use std::sync::{
    Arc,
    mpsc::{Receiver, sync_channel, SyncSender}, Mutex,
};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use byteorder::{ByteOrder, LittleEndian};
use librespot::audio::AudioPacket;
use librespot::connect::spirc::Spirc;
use librespot::core::{
    authentication::Credentials,
    cache::Cache,
    config::{ConnectConfig, DeviceType, SessionConfig, VolumeCtrl},
    session::Session,
};
use librespot::playback::{
    audio_backend,
    config::{NormalisationMethod, NormalisationType},
    config::Bitrate,
    config::PlayerConfig,
    mixer::{AudioFilter, Mixer, MixerConfig},
    player::{Player, PlayerEventChannel},
};
use librespot::protocol::authentication::AuthenticationType;
use songbird::tracks::TrackCommand::Volume;
use spotify_oauth::{SpotifyAuth, SpotifyCallback, SpotifyScope};

pub struct SpotifyPlayer {
    player_config: PlayerConfig,
    pub emitted_sink: EmittedSink,
    pub session: Session,
    pub spirc: Option<Box<Spirc>>,
    pub event_channel: Option<Arc<tokio::sync::Mutex<PlayerEventChannel>>>,
}

pub struct EmittedSink {
    sender: Arc<SyncSender<u8>>,
    pub receiver: Arc<Mutex<Receiver<u8>>>,
}

impl EmittedSink {
    fn new() -> EmittedSink {
        let (sender, receiver) = sync_channel::<u8>(64);

        EmittedSink {
            sender: Arc::new(sender),
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }
}

pub struct SoftMixer {
    volume: Arc<AtomicUsize>,
}

impl Mixer for SoftMixer {
    fn open(_: Option<MixerConfig>) -> SoftMixer {
        SoftMixer {
            volume: Arc::new(AtomicUsize::new(0xFFFF)),
        }
    }
    fn start(&self) {}
    fn stop(&self) {}
    fn volume(&self) -> u16 {
        println!("volume fetched");
        self.volume.load(Ordering::Relaxed) as u16
    }
    fn set_volume(&self, volume: u16) {
        println!("volume changed");
        self.volume.store(volume as usize, Ordering::Relaxed);
    }
    fn get_audio_filter(&self) -> Option<Box<dyn AudioFilter + Send>> {
        Some(Box::new(SoftVolumeApplier {
            volume: self.volume.clone(),
        }))
    }
}

struct SoftVolumeApplier {
    volume: Arc<AtomicUsize>,
}

impl AudioFilter for SoftVolumeApplier {
    fn modify_stream(&self, data: &mut [f32]) {
        let volume = self.volume.load(Ordering::Relaxed) as u16;
        if volume != 0xFFFF {
            let volume_factor = volume as f64 / 0xFFFF as f64;
            for x in data.iter_mut() {
                *x = (*x as f64 * volume_factor) as f32;
            }
        }
    }
}

impl audio_backend::Sink for EmittedSink {
    fn start(&mut self) -> std::result::Result<(), std::io::Error> {
        Ok(())
    }

    fn stop(&mut self) -> std::result::Result<(), std::io::Error> {
        Ok(())
    }

    fn write(&mut self, packet: &AudioPacket) -> std::result::Result<(), std::io::Error> {
        let resampled = samplerate::convert(
            44100,
            48000,
            2,
            samplerate::ConverterType::Linear,
            packet.samples(),
        )
            .unwrap();

        let sender = self.sender.clone();

        for i in resampled {
            let mut new = [0, 0, 0, 0];

            LittleEndian::write_f32_into(&[i], &mut new);

            for j in new.iter() {
                sender.send(*j).unwrap();
            }
        }

        Ok(())
    }
}

impl io::Read for EmittedSink {
    fn read(&mut self, buff: &mut [u8]) -> Result<usize, io::Error> {
        let receiver = self.receiver.lock().unwrap();

        #[allow(clippy::needless_range_loop)]
        for i in 0..buff.len() {
            buff[i] = receiver.recv().unwrap();
        }

        Ok(buff.len())
    }
}

impl Clone for EmittedSink {
    fn clone(&self) -> EmittedSink {
        EmittedSink {
            receiver: self.receiver.clone(),
            sender: self.sender.clone(),
        }
    }
}

/*pub struct SpotifyPlayerKey;
impl TypeMapKey for SpotifyPlayerKey {
    type Value = Arc<tokio::sync::Mutex<SpotifyPlayer>>;
}
*/
impl SpotifyPlayer {
    pub async fn new(
        quality: Bitrate,
        cache_dir: Option<String>,
    ) -> SpotifyPlayer {
        let tok = match env::var("TOKEN") {
            Ok(token) => {
                token
            }
            Err(_) => {
                let auth = SpotifyAuth::new_from_env("code".into(), vec![SpotifyScope::Streaming, SpotifyScope::UserReadPlaybackState, SpotifyScope::UserModifyPlaybackState, SpotifyScope::UserReadCurrentlyPlaying], false);
                let auth_url = auth.authorize_url().expect("auth url");

                println!("{}", auth_url);

                let mut buffer = String::new();
                std::io::stdin().read_line(&mut buffer);

                // Convert the given callback URL into a token.
                let token = SpotifyCallback::from_str(buffer.trim()).unwrap()
                    .convert_into_token(auth.client_id, auth.client_secret, auth.redirect_uri).await.expect("get token");
                token.access_token
            }
        };
        let credentials = Credentials {
            username: "".into(),
            auth_type: AuthenticationType::AUTHENTICATION_SPOTIFY_TOKEN,
            auth_data: tok.into_bytes(),
        };

        let session_config = SessionConfig::default();

        let mut cache: Option<Cache> = None;

        // 4 GB
        let mut cache_limit: u64 = 10;
        cache_limit = cache_limit.pow(10);
        cache_limit *= 4;

        if let Ok(c) = Cache::new(cache_dir.clone(), cache_dir, Some(cache_limit)) {
            cache = Some(c);
        }

        let session = Session::connect(session_config, credentials, cache)
            .await
            .expect("Error creating session");

        let player_config = PlayerConfig {
            bitrate: quality,
            normalisation: false,
            normalisation_type: NormalisationType::default(),
            normalisation_method: NormalisationMethod::default(),
            normalisation_pregain: 0.0,
            normalisation_threshold: -1.0,
            normalisation_attack: 0.005,
            normalisation_release: 0.1,
            normalisation_knee: 1.0,
            gapless: true,
            passthrough: false,
        };

        let emitted_sink = EmittedSink::new();

        let cloned_sink = emitted_sink.clone();

        let (_player, rx) = Player::new(player_config.clone(), session.clone(), None, move || {
            Box::new(cloned_sink)
        });

        SpotifyPlayer {
            player_config,
            emitted_sink,
            session,
            spirc: None,
            event_channel: Some(Arc::new(tokio::sync::Mutex::new(rx))),
        }
    }

    pub async fn enable_connect(&mut self) {
        let config = ConnectConfig {
            name: "Pog ass bot".to_string(),
            device_type: DeviceType::AudioDongle,
            volume: std::u16::MAX / 2,
            autoplay: true,
            volume_ctrl: VolumeCtrl::default(),
        };

        let mixer = Box::new(SoftMixer { volume: Arc::new(Default::default()) });

        let cloned_sink = self.emitted_sink.clone();

        let (player, player_events) = Player::new(
            self.player_config.clone(),
            self.session.clone(),
            mixer.get_audio_filter(),
            move || Box::new(cloned_sink),
        );

        let cloned_session = self.session.clone();

        let (spirc, task) = Spirc::new(config, cloned_session, player, mixer);

        let handle = tokio::runtime::Handle::current();
        handle.spawn(async {
            task.await;
        });

        self.spirc = Some(Box::new(spirc));

        let mut channel_lock = self.event_channel.as_ref().unwrap().lock().await;
        *channel_lock = player_events;
    }

    pub async fn disable_connect(&mut self) {
        if let Some(spirc) = self.spirc.as_ref() {
            spirc.shutdown();

            self.event_channel.as_ref().unwrap().lock().await.close();
        }
    }
}
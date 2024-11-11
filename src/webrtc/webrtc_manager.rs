use std::{error::Error, sync::Arc};

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc::UnboundedSender, Mutex, RwLock};
use tokio_tungstenite::tungstenite::Message;
use webrtc::{
    api::{
        interceptor_registry::register_default_interceptors, media_engine::MediaEngine, APIBuilder,
    },
    ice_transport::{
        ice_candidate::{RTCIceCandidate, RTCIceCandidateInit},
        ice_server::RTCIceServer,
    },
    interceptor::registry::Registry,
    peer_connection::{
        configuration::RTCConfiguration, policy::ice_transport_policy::RTCIceTransportPolicy,
        sdp::session_description::RTCSessionDescription, RTCPeerConnection,
    },
    rtp_transceiver::{
        rtp_codec::RTCRtpCodecCapability, rtp_transceiver_direction::RTCRtpTransceiverDirection,
        RTCRtpEncodingParameters, RTCRtpTransceiverInit,
    },
    track::{
        track_local::{track_local_static_sample::TrackLocalStaticSample, TrackLocal},
        track_remote::TrackRemote,
    },
};

pub struct WebRTCClient {
    peer_connection: Option<Arc<RwLock<RTCPeerConnection>>>,
}

impl WebRTCClient {
    pub fn new() -> WebRTCClient {
        WebRTCClient {
            peer_connection: None,
        }
    }

    pub fn get_peer_connection_is_none(&self) -> bool {
        self.peer_connection.is_none()
    }

    pub async fn create_peer_connection(&mut self) {
        let mut media_engine = MediaEngine::default();

        media_engine.register_default_codecs().unwrap();

        let mut registry = Registry::new();

        registry = register_default_interceptors(registry, &mut media_engine).unwrap();

        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .build();

        let stun_server_1 = RTCIceServer {
            urls: vec!["stun:stun.cloudflare.com:3478".to_string()],
            ..Default::default()
        };

        let stun_server_2 = RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_string()],
            ..Default::default()
        };

        let config = RTCConfiguration {
            ice_servers: vec![stun_server_1, stun_server_2],
            ice_transport_policy: RTCIceTransportPolicy::All,
            ..Default::default()
        };

        if let Ok(peer_connection) = api.new_peer_connection(config).await {
            // Başlangıçta boş transceiver'lar ekle
            let init = RTCRtpTransceiverInit {
                direction: RTCRtpTransceiverDirection::Sendrecv,
                send_encodings: vec![RTCRtpEncodingParameters {
                    rid: "q_360".into(),
                    ..Default::default()
                }],
            };

            // Video için boş transceiver
            peer_connection
                .add_transceiver_from_kind(
                    webrtc::rtp_transceiver::rtp_codec::RTPCodecType::Video,
                    Some(init),
                )
                .await
                .unwrap();

            let init = RTCRtpTransceiverInit {
                direction: RTCRtpTransceiverDirection::Sendrecv,
                send_encodings: vec![RTCRtpEncodingParameters {
                    ..Default::default()
                }],
            };

            // Audio için boş transceiver
            peer_connection
                .add_transceiver_from_kind(
                    webrtc::rtp_transceiver::rtp_codec::RTPCodecType::Audio,
                    Some(init),
                )
                .await
                .unwrap();
            let peer_connection = Arc::new(RwLock::new(peer_connection));
            self.peer_connection = Some(peer_connection);
        }
    }

    pub async fn set_remote_description(&mut self, sdp: String, answer_or_offer: bool) {
        let sdp = if answer_or_offer {
            RTCSessionDescription::offer(sdp).unwrap()
        } else {
            RTCSessionDescription::answer(sdp).unwrap()
        };

        self.peer_connection
            .as_mut()
            .unwrap()
            .write()
            .await
            .set_remote_description(sdp)
            .await
            .unwrap();
    }

    pub async fn create_answer(&mut self) -> Result<String, Box<dyn Error>> {
        let answer = self
            .peer_connection
            .as_mut()
            .unwrap()
            .write()
            .await
            .create_answer(None)
            .await?;
        let _ = self
            .peer_connection
            .as_mut()
            .unwrap()
            .write()
            .await
            .set_local_description(answer.clone())
            .await;
        Ok(answer.sdp)
    }

    pub async fn _create_offer(&mut self) -> Result<String, Box<dyn Error>> {
        let offer = self
            .peer_connection
            .as_mut()
            .unwrap()
            .write()
            .await
            .create_offer(None)
            .await?;
        let _ = self
            .peer_connection
            .as_mut()
            .unwrap()
            .write()
            .await
            .set_local_description(offer.clone())
            .await;
        Ok(offer.sdp)
    }

    pub async fn add_ice_candidate(
        &mut self,
        content: String,
        sdp_mid: Option<String>,
        sdp_mline_index: Option<u16>,
        username_fragment: Option<String>,
    ) {
        let candidate = RTCIceCandidateInit {
            candidate: content,
            sdp_mid,
            sdp_mline_index,
            username_fragment,
        };

        self.peer_connection
            .as_mut()
            .unwrap()
            .write()
            .await
            .add_ice_candidate(candidate)
            .await
            .unwrap();
    }

    /*pub async fn gather_and_send_candidates<F>(&mut self, callback: F)
    where
        F: FnMut(RTCIceCandidate) + Send + Sync + 'static,
    {
        let callback = Arc::new(Mutex::new(callback));
        let callback_clone = Arc::clone(&callback);

        self.peer_connection
            .as_ref()
            .unwrap()
            .on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
                let callback = Arc::clone(&callback_clone);
                Box::pin(async move {
                    if let Some(candidate) = candidate {
                        let mut callback = callback.lock().await;
                        callback(candidate.clone());
                    }
                })
            }));
    }*/

    pub async fn gather_and_send_candidates(
        &mut self,
        client: Arc<Mutex<UnboundedSender<Message>>>,
    ) {
        self.peer_connection
            .as_mut()
            .unwrap()
            .write()
            .await
            .on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
                let client = Arc::clone(&client);
                Box::pin(async move {
                    if let Some(candidate) = candidate {
                        let client = client.lock().await;
                        let candidate_msg = IceCandidateSignalMessage {
                            msg_type: "Candidate".to_string(),
                            content: candidate.to_json().unwrap().candidate,
                            sdp_mid: candidate.to_json().unwrap().sdp_mid,
                            sdp_mline_index: candidate.to_json().unwrap().sdp_mline_index,
                            username_fragment: candidate.to_json().unwrap().username_fragment,
                        };
                        let json_msg = serde_json::to_string(&candidate_msg).unwrap();
                        if let Err(e) = client.send(Message::Text(json_msg)) {
                            println!("Aday gönderme hatası: {}", e);
                        }
                    }
                })
            }));
    }

    /*pub async fn on_track(&self) {
        let pc = Arc::clone(&self.peer_connection.clone().unwrap());
        let pc_clone = Arc::clone(&pc);
        pc.read().await.on_track(Box::new(move |track, _, _| {
            let pc = Arc::clone(&pc_clone);
            Box::pin(async move {
                let local_track = match Self::convert_to_local(track.clone()).await {
                    Ok(local) => local,
                    Err(e) => {
                        println!("Track conversion error: {}", e);
                        return;
                    }
                };

                let init = RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Sendonly,
                    send_encodings: vec![],
                };

                pc.write()
                    .await
                    .add_transceiver_from_track(local_track.clone(), Some(init))
                    .await
                    .unwrap();
            })
        }));
    }*/

    pub async fn on_track(&self) {
        let pc = Arc::clone(&self.peer_connection.clone().unwrap());
        let pc_clone = Arc::clone(&pc);
        pc.read().await.on_track(Box::new(move |track, _, _| {
            let pc = Arc::clone(&pc_clone);
            Box::pin(async move {
                let transceivers = pc.write().await.get_transceivers().await;

                let new_track = Arc::new(TrackLocalStaticSample::new(
                    track.codec().capability,
                    track.id(),
                    track.stream_id(),
                ));
                for transceiver in transceivers {
                    // Eğer bu transceiver boşsa ve media type'ı uyuşuyorsa
                    if transceiver.sender().await.track().await.is_none()
                        && transceiver.kind() == new_track.kind()
                    {
                        // Track'i bu transceiver'a ekle
                        transceiver
                            .sender()
                            .await
                            .replace_track(Some(new_track))
                            .await
                            .unwrap();
                        break;
                    }
                }
            })
        }));
    }

    async fn convert_to_local(
        remote: Arc<TrackRemote>,
    ) -> Result<Arc<TrackLocalStaticSample>, Box<dyn std::error::Error>> {
        let codec = remote.codec();

        let local = Arc::new(TrackLocalStaticSample::new(
            codec.capability,
            remote.id(),
            remote.stream_id(),
        ));

        let local_clone = Arc::clone(&local);
        tokio::spawn(async move {
            while let Ok((packet, _)) = remote.read_rtp().await {
                let sample = webrtc::media::Sample {
                    data: packet.payload,
                    timestamp: std::time::UNIX_EPOCH
                        + std::time::Duration::from_secs(packet.header.timestamp as u64),
                    ..Default::default()
                };
                if let Err(e) = local_clone.write_sample(&sample).await {
                    println!("Sample write error: {}", e);
                    break;
                }
            }
        });

        Ok(local)
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct IceCandidateSignalMessage {
    msg_type: String,
    content: String,
    sdp_mid: Option<String>,
    sdp_mline_index: Option<u16>,
    username_fragment: Option<String>,
}

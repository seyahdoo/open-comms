use common::ChannelId;
use serde_json::Value;
use std::{
    collections::VecDeque,
    io::ErrorKind,
    net::{SocketAddr, UdpSocket},
    sync::{Arc, Weak},
    time::{Duration, Instant},
};
use str0m::{
    change::{SdpAnswer, SdpOffer, SdpPendingOffer},
    media::{Direction, KeyframeRequest, MediaData, MediaKind, Mid, Rid},
    net::{Protocol, Receive},
    Candidate, Event, IceConnectionState, Input, Output, Rtc, RtcError,
};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};
use uuid::Uuid;

pub type ParticipantId = Uuid;

#[derive(Debug, Clone)]
pub struct MediaConfig {
    pub bind_addr: SocketAddr,
    pub advertise_addr: SocketAddr,
}

#[allow(dead_code)]
#[derive(Debug)]
pub enum MediaSignal {
    Offer(Value),
    Answer(Value),
    IceCandidate(Value),
    Error(String),
}

pub enum MediaCommand {
    Join {
        participant_id: ParticipantId,
        channel_id: ChannelId,
        sdp: Value,
        signal_tx: mpsc::UnboundedSender<MediaSignal>,
        response: oneshot::Sender<Result<Value, String>>,
    },
    Answer {
        participant_id: ParticipantId,
        sdp: Value,
        response: oneshot::Sender<Result<(), String>>,
    },
    Offer {
        participant_id: ParticipantId,
        sdp: Value,
        response: oneshot::Sender<Result<(), String>>,
    },
    Ice {
        participant_id: ParticipantId,
        candidate: Value,
        response: oneshot::Sender<Result<(), String>>,
    },
    Leave {
        participant_id: ParticipantId,
        response: oneshot::Sender<()>,
    },
}

#[derive(Clone)]
pub struct MediaEngineHandle {
    tx: mpsc::Sender<MediaCommand>,
}

impl MediaEngineHandle {
    pub async fn join(
        &self,
        participant_id: ParticipantId,
        channel_id: ChannelId,
        sdp: Value,
    ) -> anyhow::Result<(Value, mpsc::UnboundedReceiver<MediaSignal>)> {
        let (signal_tx, signal_rx) = mpsc::unbounded_channel();
        let (response_tx, response_rx) = oneshot::channel();
        self.tx
            .send(MediaCommand::Join {
                participant_id,
                channel_id,
                sdp,
                signal_tx,
                response: response_tx,
            })
            .await?;
        let answer = response_rx
            .await
            .map_err(|e| anyhow::anyhow!("media engine dropped join request: {e}"))?
            .map_err(anyhow::Error::msg)?;
        Ok((answer, signal_rx))
    }

    pub async fn answer(&self, participant_id: ParticipantId, sdp: Value) -> Result<(), String> {
        let (response_tx, response_rx) = oneshot::channel();
        self.tx
            .send(MediaCommand::Answer {
                participant_id,
                sdp,
                response: response_tx,
            })
            .await
            .map_err(|e| e.to_string())?;
        response_rx.await.map_err(|e| e.to_string())?
    }

    pub async fn offer(&self, participant_id: ParticipantId, sdp: Value) -> Result<(), String> {
        let (response_tx, response_rx) = oneshot::channel();
        self.tx
            .send(MediaCommand::Offer {
                participant_id,
                sdp,
                response: response_tx,
            })
            .await
            .map_err(|e| e.to_string())?;
        response_rx.await.map_err(|e| e.to_string())?
    }

    pub async fn ice(
        &self,
        participant_id: ParticipantId,
        candidate: Value,
    ) -> Result<(), String> {
        let (response_tx, response_rx) = oneshot::channel();
        self.tx
            .send(MediaCommand::Ice {
                participant_id,
                candidate,
                response: response_tx,
            })
            .await
            .map_err(|e| e.to_string())?;
        response_rx.await.map_err(|e| e.to_string())?
    }

    pub async fn leave(&self, participant_id: ParticipantId) {
        let (response_tx, response_rx) = oneshot::channel();
        if self
            .tx
            .send(MediaCommand::Leave {
                participant_id,
                response: response_tx,
            })
            .await
            .is_ok()
        {
            let _ = response_rx.await;
        }
    }
}

pub fn start(config: &MediaConfig) -> anyhow::Result<MediaEngineHandle> {
    let socket = UdpSocket::bind(config.bind_addr)?;
    let advertise_addr = config.advertise_addr;
    if advertise_addr.ip().is_loopback() || advertise_addr.ip().is_unspecified() {
        warn!(
            "MEDIA_ADVERTISE_ADDR={} is loopback/unspecified; browsers will ignore this ICE candidate. Set it to a client-reachable IP.",
            advertise_addr
        );
    }
    let (tx, rx) = mpsc::channel(64);

    std::thread::Builder::new()
        .name("open-comms-media".to_string())
        .spawn(move || {
            if let Err(err) = run(socket, advertise_addr, rx) {
                warn!("media engine stopped: {err:?}");
            }
        })?;

    Ok(MediaEngineHandle { tx })
}

fn run(socket: UdpSocket, advertise_addr: SocketAddr, mut commands: mpsc::Receiver<MediaCommand>) -> Result<(), RtcError> {
    let mut clients: Vec<Client> = Vec::new();
    let mut queue: VecDeque<Propagated> = VecDeque::new();
    let mut buf = vec![0u8; 2000];

    loop {
        clients.retain(|client| client.rtc.is_alive());

        if let Ok(command) = commands.try_recv() {
            handle_command(command, &mut clients, advertise_addr);
            for client in &mut clients {
                poll_until_timeout(client, &mut queue, &socket);
            }
        }

        let mut timeout = Instant::now() + Duration::from_millis(100);
        for client in &mut clients {
            let client_timeout = poll_until_timeout(client, &mut queue, &socket);
            timeout = timeout.min(client_timeout);
        }

        if let Some(propagated) = queue.pop_front() {
            propagate(&propagated, &mut clients);
            for client in &mut clients {
                poll_until_timeout(client, &mut queue, &socket);
            }
            continue;
        }

        // Keep the socket wait short so WebRTC signaling commands are handled
        // promptly even when a client's next str0m timer is far in the future.
        let duration = (timeout - Instant::now())
            .min(Duration::from_millis(50))
            .max(Duration::from_millis(1));
        socket.set_read_timeout(Some(duration)).ok();

        if let Some(input) = read_socket_input(&socket, &mut buf, advertise_addr) {
            if let Some(client) = clients.iter_mut().find(|client| client.rtc.accepts(&input)) {
                if let Err(err) = client.rtc.handle_input(input) {
                    warn!("client {} input failed: {err:?}", client.id);
                    client.rtc.disconnect();
                }
                poll_until_timeout(client, &mut queue, &socket);
            } else {
                debug!("no client accepted UDP input");
            }
        }

        let now = Instant::now();
        for client in &mut clients {
            if let Err(err) = client.rtc.handle_input(Input::Timeout(now)) {
                warn!("client {} timeout failed: {err:?}", client.id);
                client.rtc.disconnect();
            }
            poll_until_timeout(client, &mut queue, &socket);
        }
    }
}

fn handle_command(
    command: MediaCommand,
    clients: &mut Vec<Client>,
    advertise_addr: SocketAddr,
) {
    match command {
        MediaCommand::Join {
            participant_id,
            channel_id,
            sdp,
            signal_tx,
            response,
        } => {
            let result = create_client(participant_id, channel_id, sdp, signal_tx, advertise_addr);
            match result {
                Ok((answer, client)) => {
                    let track_refs: Vec<Weak<TrackIn>> = clients
                        .iter()
                        .filter(|other| {
                            other.channel_id == channel_id && other.id != participant_id
                        })
                        .flat_map(|other| other.tracks_in.iter().map(|track| Arc::downgrade(&track.id)))
                        .collect();

                    clients.push(client);
                    let index = clients.len() - 1;
                    for track in track_refs {
                        clients[index].handle_track_open(track);
                    }

                    let _ = response.send(Ok(answer));
                }
                Err(err) => {
                    let _ = response.send(Err(err));
                }
            }
        }
        MediaCommand::Answer {
            participant_id,
            sdp,
            response,
        } => {
            let result = (|| -> Result<(), String> {
                let answer: SdpAnswer = serde_json::from_value(sdp).map_err(|e| e.to_string())?;
                let client = clients
                    .iter_mut()
                    .find(|client| client.id == participant_id)
                    .ok_or("participant not found")?;
                client.handle_answer(answer);
                Ok(())
            })();
            let _ = response.send(result);
        }
        MediaCommand::Offer {
            participant_id,
            sdp,
            response,
        } => {
            let result = (|| -> Result<(), String> {
                let offer: SdpOffer = serde_json::from_value(sdp).map_err(|e| e.to_string())?;
                let client = clients
                    .iter_mut()
                    .find(|client| client.id == participant_id)
                    .ok_or("participant not found")?;
                client.handle_offer(offer);
                Ok(())
            })();
            let _ = response.send(result);
        }
        MediaCommand::Ice {
            participant_id,
            candidate,
            response,
        } => {
            let result = (|| -> Result<(), String> {
                if let Some(candidate) = parse_ice_candidate(&candidate)? {
                    let client = clients
                        .iter_mut()
                        .find(|client| client.id == participant_id)
                        .ok_or("participant not found")?;
                    client.rtc.add_remote_candidate(candidate);
                }
                Ok(())
            })();
            let _ = response.send(result);
        }
        MediaCommand::Leave {
            participant_id,
            response,
        } => {
            clients.retain(|client| client.id != participant_id);
            let _ = response.send(());
        }
    }
}

fn parse_ice_candidate(value: &Value) -> Result<Option<Candidate>, String> {
    let raw = match value {
        Value::String(raw) => raw.clone(),
        Value::Object(map) => {
            if let Some(candidate) = map.get("candidate").and_then(Value::as_str) {
                candidate.to_string()
            } else if let Some((key, candidate)) = map.iter().find(|(_, value)| value.is_string()) {
                format!("{key}={candidate}")
            } else {
                return Err("unsupported ICE candidate shape".to_string());
            }
        }
        _ => return Err("unsupported ICE candidate shape".to_string()),
    };

    // Browsers send the candidate value from `RTCIceCandidate.toJSON()`, which
    // omits the `candidate:` SDP attribute prefix that str0m expects.
    let raw = raw.trim();
    if raw.is_empty() {
        // End-of-candidates signal: str0m has no remote candidate for this case.
        return Ok(None);
    }
    let raw = raw.strip_prefix("a=").unwrap_or(raw).trim();
    let sdp = if raw.starts_with("candidate:") {
        raw.to_string()
    } else {
        format!("candidate:{raw}")
    };

    Candidate::from_sdp_string(&sdp)
        .map(Some)
        .map_err(|e| e.to_string())
}

fn create_client(
    participant_id: ParticipantId,
    channel_id: ChannelId,
    sdp: Value,
    signal_tx: mpsc::UnboundedSender<MediaSignal>,
    advertise_addr: SocketAddr,
) -> Result<(Value, Client), String> {
    let offer: SdpOffer = serde_json::from_value(sdp).map_err(|e| e.to_string())?;
    let mut rtc = Rtc::builder().set_ice_lite(true).build(Instant::now());
    let candidate = Candidate::host(advertise_addr, "udp").map_err(|e| e.to_string())?;
    rtc.add_local_candidate(candidate)
        .ok_or("server ICE candidate was rejected")?;

    let answer = rtc
        .sdp_api()
        .accept_offer(offer)
        .map_err(|e| e.to_string())?;
    let answer_json = serde_json::to_value(answer).map_err(|e| e.to_string())?;

    Ok((
        answer_json,
        Client::new(participant_id, channel_id, rtc, signal_tx),
    ))
}

fn poll_until_timeout(
    client: &mut Client,
    queue: &mut VecDeque<Propagated>,
    socket: &UdpSocket,
) -> Instant {
    loop {
        if !client.rtc.is_alive() {
            return Instant::now();
        }

        let propagated = client.poll_output(socket);
        if let Propagated::Timeout(timeout) = propagated {
            return timeout;
        }

        if !matches!(propagated, Propagated::Noop) {
            queue.push_back(propagated);
        }
    }
}

fn propagate(propagated: &Propagated, clients: &mut [Client]) {
    match propagated {
        Propagated::TrackOpen {
            channel_id,
            origin,
            track,
        } => {
            for client in clients.iter_mut() {
                if client.channel_id == *channel_id && client.id != *origin {
                    client.handle_track_open(track.clone());
                }
            }
        }
        Propagated::MediaData {
            channel_id,
            origin,
            data,
        } => {
            for client in clients.iter_mut() {
                if client.channel_id == *channel_id && client.id != *origin {
                    client.handle_media_data_out(*origin, data);
                }
            }
        }
        Propagated::KeyframeRequest {
            channel_id,
            origin,
            req,
            mid,
        } => {
            for client in clients.iter_mut() {
                if client.channel_id == *channel_id && client.id == *origin {
                    client.handle_keyframe_request(req.clone(), *mid);
                }
            }
        }
        Propagated::Noop | Propagated::Timeout(_) => {}
    }
}

fn read_socket_input<'a>(
    socket: &UdpSocket,
    buf: &'a mut Vec<u8>,
    advertise_addr: SocketAddr,
) -> Option<Input<'a>> {
    buf.resize(2000, 0);
    match socket.recv_from(buf) {
        Ok((size, source)) => {
            buf.truncate(size);
            let contents = buf.as_slice().try_into().ok()?;
            Some(Input::Receive(
                Instant::now(),
                Receive {
                    proto: Protocol::Udp,
                    source,
                    // str0m validates STUN requests against local candidates. When
                    // running behind NAT/Docker port forwarding, the destination
                    // seen by the socket differs from the address advertised to
                    // browsers, so report the advertised address here.
                    destination: advertise_addr,
                    contents,
                },
            ))
        }
        Err(err) if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => None,
        Err(err) => {
            warn!("UDP read failed: {err}");
            None
        }
    }
}

struct Client {
    id: ParticipantId,
    channel_id: ChannelId,
    rtc: Rtc,
    pending: Option<SdpPendingOffer>,
    signal_tx: mpsc::UnboundedSender<MediaSignal>,
    tracks_in: Vec<TrackInEntry>,
    tracks_out: Vec<TrackOut>,
    chosen_rid: Option<Rid>,
}

impl Client {
    fn new(
        id: ParticipantId,
        channel_id: ChannelId,
        rtc: Rtc,
        signal_tx: mpsc::UnboundedSender<MediaSignal>,
    ) -> Self {
        Self {
            id,
            channel_id,
            rtc,
            pending: None,
            signal_tx,
            tracks_in: Vec::new(),
            tracks_out: Vec::new(),
            chosen_rid: None,
        }
    }

    fn poll_output(&mut self, socket: &UdpSocket) -> Propagated {
        if !self.rtc.is_alive() {
            return Propagated::Timeout(Instant::now());
        }

        let _ = self.negotiate_if_needed();

        match self.rtc.poll_output() {
            Ok(output) => self.handle_output(output, socket),
            Err(err) => {
                warn!("client {} poll_output failed: {err:?}", self.id);
                self.rtc.disconnect();
                Propagated::Noop
            }
        }
    }

    fn handle_output(&mut self, output: Output, socket: &UdpSocket) -> Propagated {
        match output {
            Output::Transmit(transmit) => {
                if let Err(err) = socket.send_to(&transmit.contents, transmit.destination) {
                    warn!("failed to send UDP media: {err}");
                }
                Propagated::Noop
            }
            Output::Timeout(timeout) => Propagated::Timeout(timeout),
            Output::Event(event) => match event {
                Event::IceConnectionStateChange(state) => {
                    info!("client {} ICE state: {:?}", self.id, state);
                    if state == IceConnectionState::Disconnected {
                        self.rtc.disconnect();
                    }
                    Propagated::Noop
                }
                Event::MediaAdded(added) => self.handle_media_added(added.mid, added.kind),
                Event::MediaData(data) => self.handle_media_data_in(data),
                Event::KeyframeRequest(req) => self.handle_incoming_keyframe_req(req),
                _ => Propagated::Noop,
            },
        }
    }

    fn handle_media_added(&mut self, mid: Mid, kind: MediaKind) -> Propagated {
        let track = TrackInEntry {
            id: Arc::new(TrackIn {
                origin: self.id,
                mid,
                kind,
            }),
            last_keyframe_request: None,
        };
        let weak = Arc::downgrade(&track.id);
        self.tracks_in.push(track);

        Propagated::TrackOpen {
            channel_id: self.channel_id,
            origin: self.id,
            track: weak,
        }
    }

    fn handle_media_data_in(&mut self, data: MediaData) -> Propagated {
        if !data.contiguous {
            self.request_keyframe_throttled(data.mid, data.rid);
        }
        Propagated::MediaData {
            channel_id: self.channel_id,
            origin: self.id,
            data,
        }
    }

    fn request_keyframe_throttled(&mut self, mid: Mid, rid: Option<Rid>) {
        let Some(mut writer) = self.rtc.writer(mid) else {
            return;
        };
        let Some(track) = self.tracks_in.iter_mut().find(|track| track.id.mid == mid) else {
            return;
        };
        if track
            .last_keyframe_request
            .map(|time| time.elapsed() < Duration::from_secs(1))
            .unwrap_or(false)
        {
            return;
        }
        _ = writer.request_keyframe(rid, str0m::media::KeyframeRequestKind::Fir);
        track.last_keyframe_request = Some(Instant::now());
    }

    fn handle_incoming_keyframe_req(&self, req: KeyframeRequest) -> Propagated {
        let Some(track_out) = self
            .tracks_out
            .iter()
            .find(|track| track.mid() == Some(req.mid))
        else {
            return Propagated::Noop;
        };
        let Some(track_in) = track_out.track_in.upgrade() else {
            return Propagated::Noop;
        };

        Propagated::KeyframeRequest {
            channel_id: self.channel_id,
            origin: track_in.origin,
            req,
            mid: track_in.mid,
        }
    }

    fn handle_track_open(&mut self, track_in: Weak<TrackIn>) {
        self.tracks_out.push(TrackOut {
            track_in,
            state: TrackOutState::ToOpen,
        });
    }

    fn handle_media_data_out(&mut self, origin: ParticipantId, data: &MediaData) {
        let Some(mid) = self
            .tracks_out
            .iter()
            .find(|track| {
                track
                    .track_in
                    .upgrade()
                    .filter(|input| input.origin == origin && input.mid == data.mid)
                    .is_some()
            })
            .and_then(|track| track.mid())
        else {
            return;
        };

        if data.rid.is_some() && data.rid != Some("h".into()) {
            return;
        }
        self.chosen_rid = data.rid;

        let Some(writer) = self.rtc.writer(mid) else {
            return;
        };
        let Some(pt) = writer.match_params(data.params) else {
            return;
        };

        if let Err(err) = writer.write(pt, data.network_time, data.time, data.data.clone()) {
            warn!("client {} media write failed: {err:?}", self.id);
            self.rtc.disconnect();
        }
    }

    fn handle_keyframe_request(&mut self, mut req: KeyframeRequest, mid_in: Mid) {
        let has_track = self.tracks_in.iter().any(|track| track.id.mid == mid_in);
        if !has_track {
            return;
        }

        req.rid = self.chosen_rid;
        let Some(mut writer) = self.rtc.writer(mid_in) else {
            return;
        };
        if let Err(err) = writer.request_keyframe(req.rid, req.kind) {
            debug!("keyframe request failed: {err:?}");
        }
    }

    fn negotiate_if_needed(&mut self) -> bool {
        if self.pending.is_some() {
            return false;
        }

        for track in &mut self.tracks_out {
            if let TrackOutState::Open(mid) = track.state {
                if track.track_in.upgrade().is_none() {
                    track.state = TrackOutState::ToStop(mid);
                }
            }
        }

        let mut change = self.rtc.sdp_api();
        for track in &mut self.tracks_out {
            match track.state {
                TrackOutState::ToOpen => {
                    if let Some(track_in) = track.track_in.upgrade() {
                        let stream_id = track_in.origin.to_string();
                        let mid = change.add_media(
                            track_in.kind,
                            Direction::SendOnly,
                            Some(stream_id),
                            None,
                            None,
                        );
                        track.state = TrackOutState::Negotiating(mid);
                    }
                }
                TrackOutState::ToStop(mid) => {
                    change.stop_media(mid);
                    track.state = TrackOutState::NegotiatingStop(mid);
                }
                _ => {}
            }
        }

        if !change.has_changes() {
            return false;
        }

        let Some((offer, pending)) = change.apply() else {
            return false;
        };
        let Ok(json) = serde_json::to_value(&offer) else {
            return false;
        };

        let _ = self.signal_tx.send(MediaSignal::Offer(json));
        self.pending = Some(pending);
        true
    }

    fn handle_offer(&mut self, offer: SdpOffer) {
        let answer = match self.rtc.sdp_api().accept_offer(offer) {
            Ok(answer) => answer,
            Err(err) => {
                warn!("client {} offer rejected: {err:?}", self.id);
                let _ = self
                    .signal_tx
                    .send(MediaSignal::Error("failed to accept offer".into()));
                return;
            }
        };

        // An incoming offer invalidates any local pending offer; those track
        // changes will be recreated after this negotiation completes.
        self.pending = None;
        for track in &mut self.tracks_out {
            match track.state {
                TrackOutState::Negotiating(_) => track.state = TrackOutState::ToOpen,
                TrackOutState::NegotiatingStop(mid) => track.state = TrackOutState::ToStop(mid),
                _ => {}
            }
        }

        if let Ok(json) = serde_json::to_value(&answer) {
            let _ = self.signal_tx.send(MediaSignal::Answer(json));
        }
    }

    fn handle_answer(&mut self, answer: SdpAnswer) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        if self.rtc.sdp_api().accept_answer(pending, answer).is_err() {
            let _ = self
                .signal_tx
                .send(MediaSignal::Error("failed to accept answer".into()));
            return;
        }

        for track in &mut self.tracks_out {
            if let TrackOutState::Negotiating(mid) = track.state {
                track.state = TrackOutState::Open(mid);
            }
        }
        self.tracks_out
            .retain(|track| !matches!(track.state, TrackOutState::NegotiatingStop(_)));
    }
}

#[derive(Debug)]
struct TrackIn {
    origin: ParticipantId,
    mid: Mid,
    kind: MediaKind,
}

#[derive(Debug)]
struct TrackInEntry {
    id: Arc<TrackIn>,
    last_keyframe_request: Option<Instant>,
}

#[derive(Debug)]
struct TrackOut {
    track_in: Weak<TrackIn>,
    state: TrackOutState,
}

#[derive(Debug, Clone, Copy)]
enum TrackOutState {
    ToOpen,
    Negotiating(Mid),
    Open(Mid),
    ToStop(Mid),
    NegotiatingStop(Mid),
}

impl TrackOut {
    fn mid(&self) -> Option<Mid> {
        match self.state {
            TrackOutState::ToOpen => None,
            TrackOutState::Negotiating(mid)
            | TrackOutState::Open(mid)
            | TrackOutState::ToStop(mid)
            | TrackOutState::NegotiatingStop(mid) => Some(mid),
        }
    }
}

#[derive(Debug)]
enum Propagated {
    Noop,
    Timeout(Instant),
    TrackOpen {
        channel_id: ChannelId,
        origin: ParticipantId,
        track: Weak<TrackIn>,
    },
    MediaData {
        channel_id: ChannelId,
        origin: ParticipantId,
        data: MediaData,
    },
    KeyframeRequest {
        channel_id: ChannelId,
        origin: ParticipantId,
        req: KeyframeRequest,
        mid: Mid,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_browser_candidate_without_sdp_prefix() {
        let value = serde_json::json!({
            "candidate": "2679991984 1 tcp 1518285567 fd92:5902:196f:81ff:6e3f:df83:6933:3139 9 typ host tcptype active generation 0 ufrag 6a/J network-id 6 network-cost 50",
            "sdpMid": "0",
            "sdpMLineIndex": 0,
        });

        assert!(parse_ice_candidate(&value).unwrap().is_some());
    }

    #[test]
    fn accepts_empty_candidate_as_end_of_candidates() {
        let value = serde_json::json!({ "candidate": "" });
        assert!(parse_ice_candidate(&value).unwrap().is_none());
    }
}

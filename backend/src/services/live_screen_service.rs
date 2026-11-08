use crate::{
    Result,
    dto::live_screen::{Signal, View},
    error,
};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use uuid::Uuid;
#[derive(Default)]
struct Room {
    id: Uuid,
    offer: Option<String>,
    answer: Option<String>,
    heartbeat: Option<Instant>,
}
static ROOMS: OnceLock<Mutex<HashMap<Uuid, Room>>> = OnceLock::new();
pub(crate) fn exchange(session: Uuid, signal: Signal, candidate: bool) -> Result<View> {
    if signal
        .sdp
        .as_ref()
        .is_some_and(|s| s.len() > 65536)
    {
        return Err(error::bad("SDP is too large"));
    }
    let mut rooms = ROOMS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| error::conflict("Signaling unavailable"))?;
    rooms.retain(|_, room| {
        room.heartbeat
            .is_some_and(|t| t.elapsed() < Duration::from_secs(20))
    });
    if signal.action == "start" && !candidate {
        if rooms.contains_key(&session) {
            return Err(error::conflict("A live viewer is already connected"));
        }
        if rooms.len() >= 1000 {
            return Err(error::conflict("Live view capacity reached"));
        }
        rooms.insert(
            session,
            Room {
                id: Uuid::new_v4(),
                heartbeat: Some(Instant::now()),
                ..Default::default()
            },
        );
    }
    let mut view = View {
        id: Uuid::nil(),
        offer: None,
        answer: None,
        ice_servers: serde_json::from_str(
            &std::env::var("WEBRTC_ICE_SERVERS").unwrap_or_else(|_| "[]".to_string()),
        )
        .unwrap_or(serde_json::json!([])),
    };
    if let Some(room) = rooms.get_mut(&session) {
        let matches = signal.id == Some(room.id);
        match signal.action.as_str() {
            "start" if !candidate => {}
            "poll" if candidate => {}
            "poll" if matches => {
                room.heartbeat = Some(Instant::now());
            }
            "offer" if candidate && matches && room.offer.is_none() => {
                room.offer = Some(
                    signal
                        .sdp
                        .ok_or_else(|| error::bad("Missing offer"))?,
                );
            }
            "answer" if !candidate && matches && room.answer.is_none() => {
                room.answer = Some(
                    signal
                        .sdp
                        .ok_or_else(|| error::bad("Missing answer"))?,
                );
            }
            "stop" if matches => {
                rooms.remove(&session);
                return Ok(view);
            }
            _ => return Err(error::conflict("Live view changed; reconnect")),
        }
        view.id = room.id;
        view.offer = room.offer.clone();
        view.answer = room.answer.clone();
    } else if signal.action != "poll" {
        return Err(error::conflict("Live view ended"));
    }
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn message(action: &str, id: Option<Uuid>, sdp: Option<&str>) -> Signal {
        Signal {
            action: action.to_string(),
            id,
            sdp: sdp.map(str::to_string),
        }
    }
    #[test]
    fn live_screen_signaling_is_scoped_and_role_checked() {
        let session = Uuid::new_v4();
        let other = Uuid::new_v4();
        assert!(exchange(session, message("start", None, None), true).is_err());
        let room = exchange(session, message("start", None, None), false)
            .unwrap()
            .id;
        assert!(exchange(session, message("start", None, None), false).is_err());
        assert!(
            exchange(other, message("poll", None, None), true)
                .unwrap()
                .id
                .is_nil()
        );
        assert!(exchange(session, message("offer", Some(room), Some("offer")), false).is_err());
        assert!(exchange(session, message("answer", Some(room), Some("answer")), true).is_err());
        assert!(exchange(session, message("poll", Some(Uuid::new_v4()), None), false).is_err());
        exchange(session, message("offer", Some(room), Some("offer")), true).unwrap();
        assert_eq!(
            exchange(session, message("poll", Some(room), None), false)
                .unwrap()
                .offer
                .as_deref(),
            Some("offer")
        );
        exchange(
            session,
            message("answer", Some(room), Some("answer")),
            false,
        )
        .unwrap();
        assert_eq!(
            exchange(session, message("poll", None, None), true)
                .unwrap()
                .answer
                .as_deref(),
            Some("answer")
        );
        exchange(session, message("stop", Some(room), None), false).unwrap();
        assert!(
            exchange(session, message("poll", None, None), true)
                .unwrap()
                .id
                .is_nil()
        );
    }
}

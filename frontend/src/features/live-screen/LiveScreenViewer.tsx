import { useEffect, useRef, useState } from 'react';
import { gather, hasViewer, signal } from './signaling';

export function LiveScreenViewer({ session, token }: { session: string; token: string }) {
  const [watching, setWatching] = useState(false);
  const [status, setStatus] = useState('');
  const video = useRef<HTMLVideoElement>(null);
  useEffect(() => {
    if (!watching) return;
    const path = `/api/v1/sessions/${session}/live-screen`;
    const abort = new AbortController(); let room = ''; let peer: RTCPeerConnection | null = null;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const state = await signal(path, token, room ? 'poll' : 'start', room || undefined, undefined, abort.signal);
        if (abort.signal.aborted) {
          if (hasViewer(state.id)) void signal(path, token, 'stop', state.id).catch(() => {});
          return;
        }
        if (!hasViewer(state.id)) throw new Error('Live view ended. Stop and reconnect.');
        room = state.id;
        if (state.offer && !peer) {
          peer = new RTCPeerConnection({ iceServers: state.ice_servers });
          peer.ontrack = event => {
            if (video.current) { video.current.srcObject = new MediaStream([event.track]); void video.current.play().catch(() => setStatus('Click the video play button to view.')); }
          };
          peer.onconnectionstatechange = () => {
            if (!abort.signal.aborted && peer) setStatus(peer.connectionState === 'connected' ? 'Live candidate screen · video only' : `Connection: ${peer.connectionState}`);
          };
          await peer.setRemoteDescription({ type: 'offer', sdp: state.offer });
          await peer.setLocalDescription(await peer.createAnswer());
          await gather(peer, abort.signal);
          if (abort.signal.aborted) return;
          await signal(path, token, 'answer', room, peer.localDescription!.sdp, abort.signal);
        } else if (!peer) setStatus('Waiting for the candidate’s shared screen…');
        timer = setTimeout(() => void poll(), 2000);
      } catch (error) {
        if (!abort.signal.aborted) { setStatus(error instanceof Error ? error.message : 'Unable to connect'); setWatching(false); }
      }
    }
    void poll();
    return () => {
      abort.abort(); clearTimeout(timer); peer?.close();
      if (video.current) video.current.srcObject = null;
      if (room) void signal(path, token, 'stop', room).catch(() => {});
    };
  }, [watching, session, token]);
  return <section className="invitation-panel"><h3>Live screen</h3><p>The candidate sees a live-view notice. Uses their shared screen; camera and microphone are not streamed.</p><button className="button secondary" onClick={() => { setStatus(''); setWatching(value => !value); }}>{watching ? 'Stop live view' : 'View live screen'}</button>{status && <p role="status">{status}</p>}{watching && <video ref={video} autoPlay muted playsInline controls style={{ width: '100%', maxHeight: '65vh', background: '#000', marginTop: 12 }} />}</section>;
}

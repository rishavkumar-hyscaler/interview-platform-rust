import { useEffect, useState } from 'react';
import { gather, hasViewer, signal } from './signaling';

/** Reuses the explicitly shared screen; never starts camera or microphone capture. */
export function useLiveScreenSender(token: string, active: boolean, stream: MediaStream | null) {
  const [status, setStatus] = useState('');
  useEffect(() => {
    if (!active) { setStatus(''); return; }
    const abort = new AbortController();
    let peer: RTCPeerConnection | null = null;
    let room = ''; let timer: ReturnType<typeof setTimeout>;
    const close = () => { peer?.close(); peer = null; room = ''; };
    async function poll() {
      try {
        const state = await signal('/api/v1/candidate/live-screen', token, 'poll', undefined, undefined, abort.signal);
        if (abort.signal.aborted) return;
        if (!hasViewer(state.id)) { close(); setStatus(''); }
        else if (!stream || stream.getVideoTracks()[0]?.readyState !== 'live') {
          close(); setStatus('Your interviewer requested live view. Share your screen to connect.');
        } else {
          if (room !== state.id) {
            close(); room = state.id;
            peer = new RTCPeerConnection({ iceServers: state.ice_servers });
            peer.onconnectionstatechange = () => {
              if (abort.signal.aborted || !peer) return;
              setStatus(peer.connectionState === 'connected' ? 'Your interviewer is viewing your shared screen live.' : `Live screen connection: ${peer.connectionState}`);
            };
            for (const track of stream.getVideoTracks()) peer.addTrack(track, stream);
            setStatus('Connecting your shared screen to your interviewer…');
            await peer.setLocalDescription(await peer.createOffer());
            await gather(peer, abort.signal);
            if (abort.signal.aborted) return;
            await signal('/api/v1/candidate/live-screen', token, 'offer', room, peer.localDescription!.sdp, abort.signal);
          }
          if (peer && state.answer && !peer.remoteDescription) await peer.setRemoteDescription({ type: 'answer', sdp: state.answer });
        }
      } catch (error) {
        if (!abort.signal.aborted) { close(); setStatus(error instanceof Error ? error.message : 'Live screen connection unavailable'); }
      } finally { if (!abort.signal.aborted) timer = setTimeout(() => void poll(), 2000); }
    }
    void poll();
    return () => { abort.abort(); clearTimeout(timer); if (room) void signal('/api/v1/candidate/live-screen', token, 'stop', room).catch(() => {}); close(); };
  }, [token, active, stream]);
  return status;
}

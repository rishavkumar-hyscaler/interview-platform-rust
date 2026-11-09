import { useEffect, useRef, useState } from 'react';
import { gather, hasViewer, signal } from './signaling';

/** Reuses the shared screen and enabled camera without starting device capture. */
export function useLiveScreenSender(token: string, active: boolean, stream: MediaStream | null, cameraTrack: MediaStreamTrack | null) {
  const [status, setStatus] = useState('');
  const camera = useRef(cameraTrack);
  camera.current = cameraTrack;
  useEffect(() => {
    if (!active) { setStatus(''); return; }
    const abort = new AbortController();
    let peer: RTCPeerConnection | null = null;
    let cameraSender: RTCRtpSender | null = null;
    let room = ''; let timer: ReturnType<typeof setTimeout>;
    const close = () => { peer?.close(); peer = null; cameraSender = null; room = ''; };
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
              setStatus(peer.connectionState === 'connected' ? 'Your interviewer is viewing your shared screen and any enabled camera live.' : `Live screen connection: ${peer.connectionState}`);
            };
            peer.addTrack(stream.getVideoTracks()[0], stream);
            cameraSender = peer.addTransceiver('video', { direction: 'sendonly' }).sender;
            await cameraSender.replaceTrack(camera.current);
            setStatus('Connecting your shared screen to your interviewer…');
            await peer.setLocalDescription(await peer.createOffer());
            await gather(peer, abort.signal);
            if (abort.signal.aborted) return;
            await signal('/api/v1/candidate/live-screen', token, 'offer', room, peer.localDescription!.sdp, abort.signal);
          }
          if (cameraSender && cameraSender.track !== camera.current) await cameraSender.replaceTrack(camera.current);
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

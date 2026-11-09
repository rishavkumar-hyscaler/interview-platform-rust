import { useEffect, useRef, useState } from 'react';
import { gather, hasViewer, signal } from './signaling';

export function LiveScreenViewer({ session, token }: { session: string; token: string }) {
  const [watching, setWatching] = useState(false);
  const [status, setStatus] = useState('');
  const video = useRef<HTMLVideoElement>(null);
  const camera = useRef<HTMLVideoElement>(null);
  useEffect(() => {
    if (!watching) return;
    const screenVideo = video.current;
    const cameraVideo = camera.current;
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
            const target = event.transceiver === peer?.getTransceivers()[0] ? screenVideo : cameraVideo;
            if (abort.signal.aborted || event.track.kind !== 'video' || !target) return;
            target.srcObject = new MediaStream([event.track]);
            void target.play().catch(() => {
              if (!abort.signal.aborted) setStatus('Click the video play button to view.');
            });
          };
          peer.onconnectionstatechange = () => {
            if (abort.signal.aborted || !peer) return;
            if (peer.connectionState === 'failed') {
              setStatus('Live connection lost. Start live view again to reconnect.');
              setWatching(false);
              return;
            }
            setStatus(peer.connectionState === 'connected' ? 'Live video connected' : `Connection: ${peer.connectionState}`);
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
      if (screenVideo) screenVideo.srcObject = null;
      if (cameraVideo) cameraVideo.srcObject = null;
      if (room) void signal(path, token, 'stop', room).catch(() => {});
    };
  }, [watching, session, token]);
  return <section className="invitation-panel">
    <h3>Live view</h3>
    <p>View the shared screen and the candidate’s camera when enabled. The candidate sees a live-view notice. Microphone audio is excluded.</p>
    <button className="button secondary" onClick={() => { setStatus(''); setWatching(value => !value); }}>
      {watching ? 'Stop live view' : 'Start live view'}
    </button>
    {status && <p role="status">{status}</p>}
    {watching && <div className="mt-3 grid gap-4">
      <div>
        <h4>Shared screen</h4>
        <video ref={video} className="w-full max-h-[65vh] bg-black" autoPlay muted playsInline controls />
      </div>
      <div>
        <h4>Candidate camera</h4>
        <video ref={camera} className="aspect-video w-full max-w-sm bg-black" autoPlay muted playsInline controls />
      </div>
    </div>}
  </section>;
}

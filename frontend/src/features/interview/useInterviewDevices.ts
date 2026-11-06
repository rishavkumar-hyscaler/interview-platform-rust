import { useCallback, useEffect, useRef, useState } from 'react';
import type { Observations } from './proctoring/observations';
type DeviceState = 'waiting' | 'requesting' | 'enabled' | 'denied' | 'unavailable' | 'off';
export function useInterviewDevices(session: string | undefined, active: boolean, record: Observations['record']) {
  const stream = useRef<MediaStream | null>(null);
  const alive = useRef(true); const pending = useRef(new Set<string>()); const prompted = useRef('');
  const [camera, setCamera] = useState<DeviceState>('waiting');
  const [microphone, setMicrophone] = useState<DeviceState>('waiting');
  const [revision, setRevision] = useState(0);
  const [error, setError] = useState('');
  const request = useCallback(async (kind: 'video' | 'audio') => {
    if (!session || pending.current.has(kind)) return;
    const label = kind === 'video' ? 'camera' : 'microphone';
    const update = kind === 'video' ? setCamera : setMicrophone;
    if (stream.current?.getTracks().some(track => track.kind === kind && track.readyState === 'live')) return;
    pending.current.add(kind); update('requesting'); setError(''); record(`${label}_permission_requested`);
    try {
      if (!navigator.mediaDevices?.getUserMedia) throw new Error('Camera and microphone access requires localhost or HTTPS.');
      const acquired = await navigator.mediaDevices.getUserMedia({video:kind === 'video',audio:kind === 'audio'});
      if (!alive.current) { acquired.getTracks().forEach(track => track.stop()); return; }
      if (!stream.current) stream.current = new MediaStream();
      const tracks = acquired.getTracks().filter(track => track.kind === kind && track.readyState === 'live');
      if (!tracks.length) { acquired.getTracks().forEach(track => track.stop()); throw new Error(`No working ${label} was returned.`); }
      tracks.forEach(track => {
        stream.current!.addTrack(track);
        track.onended = () => { if (alive.current) { update('off'); setRevision(value=>value+1); record(`${label}_disabled`); } };
      });
      update('enabled'); setRevision(value=>value+1); record(`${label}_enabled`);
    } catch (failure) {
      if (!alive.current) return;
      const denied = failure instanceof DOMException && failure.name === 'NotAllowedError';
      update(denied ? 'denied' : 'unavailable'); record(`${label}_${denied ? 'denied' : 'unavailable'}`);
      setError(denied ? `${label} permission was denied. This is recorded; browser monitoring continues. You can allow it in the browser address bar and retry.` : failure instanceof Error ? failure.message : `Unable to access ${label}.`);
    } finally { pending.current.delete(kind); }
  }, [session, record]);
  const toggle = useCallback(async (kind:'video'|'audio') => {
    const tracks = stream.current?.getTracks().filter(track=>track.kind===kind && track.readyState==='live') ?? [];
    if (!tracks.length) { await request(kind); return; }
    tracks.forEach(track=>{track.stop();stream.current?.removeTrack(track);});
    if (kind==='video') setCamera('off'); else setMicrophone('off');
    setRevision(value=>value+1); record(kind==='video'?'camera_disabled':'microphone_disabled');
  }, [request, record]);
  useEffect(()=>{
    alive.current=true;
    if (active && session && prompted.current!==session) {
      prompted.current=session;
      void (async()=>{await request('video'); if(alive.current) await request('audio');})();
    }
    return ()=>{alive.current=false;stream.current?.getTracks().forEach(track=>track.stop());stream.current=null;};
  },[session,active,request]);
  return {stream,camera,microphone,revision,error,request,toggle,busy:camera==='requesting'||microphone==='requesting'};
}
export type InterviewDevices = ReturnType<typeof useInterviewDevices>;

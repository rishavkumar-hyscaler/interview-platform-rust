import { useCallback, useEffect, useRef, useState } from 'react';
import { evidenceQueue, snapshot } from './proctoring/evidence';
import type { BrowserObservation, Observations } from './proctoring/observations';
const flags = new Set(['fullscreen_entered','fullscreen_exited','focus_lost','hidden','paste','copy','context_menu','devtools_suspected','multiple_displays','typing_burst','rapid_answer','camera_disabled','microphone_disabled','screen_share_started']);
export function useScreenEvidence(token:string, session:string|undefined, active:boolean, observations:Observations) {
  const stream = useRef<MediaStream|null>(null);
  const video = useRef<HTMLVideoElement|null>(null);
  const queue = useRef<ReturnType<typeof evidenceQueue>|null>(null);
  const alive = useRef(true); const entered = useRef(false); const entryRecorded = useRef(false); const isActive=useRef(active); isActive.current=active;
  const [shared,setShared]=useState(false); const [busy,setBusy]=useState(false);
  const [surface,setSurface]=useState(''); const [error,setError]=useState(''); const [status,setStatus]=useState('Screen sharing not started');
  useEffect(()=>{alive.current=true;return()=>{alive.current=false;stream.current?.getTracks().forEach(track=>track.stop());if(video.current)video.current.srcObject=null;};},[]);
  useEffect(()=>{
    if(!session)return;
    const next=evidenceQueue(token,session,setStatus);queue.current=next;
    return()=>{next.stop();queue.current=null;};
  },[token,session]);
  useEffect(()=>{
    if(active)return;
    entered.current=false;entryRecorded.current=false;
  },[active]);
  const stop = useCallback(()=>{
    stream.current?.getTracks().forEach(track=>track.stop());stream.current=null;
    if(video.current)video.current.srcObject=null;
    setShared(false);setStatus('Screen sharing stopped');
  },[]);
  const request = useCallback(async()=>{
    setBusy(true);setError('');observations.record('screen_share_requested');
    try {
      if(!navigator.mediaDevices?.getDisplayMedia)throw new Error('Screen sharing requires a supported desktop browser on localhost or HTTPS.');
      const acquired=await navigator.mediaDevices.getDisplayMedia({video:true,audio:false});
      if(!alive.current){acquired.getTracks().forEach(track=>track.stop());return;}
      const track=acquired.getVideoTracks()[0];
      if(!track){acquired.getTracks().forEach(track=>track.stop());throw new Error('No screen was shared.');}
      stream.current?.getTracks().forEach(old=>old.stop());stream.current=acquired;
      const preview=document.createElement('video');preview.muted=true;preview.playsInline=true;preview.srcObject=acquired;video.current=preview;
      await preview.play();
      const selected=track.getSettings().displaySurface ?? 'unknown';setSurface(selected);
      track.onended=()=>{if(alive.current){stop();observations.record('screen_share_stopped');}};
      setShared(true);setStatus('Screen sharing active · screenshots are saved on fullscreen entry and flagged events');
      observations.record('screen_share_started');
    } catch(failure) {
      if(!alive.current)return;
      stop();
      const denied=failure instanceof DOMException && failure.name==='NotAllowedError';
      observations.record(denied?'screen_share_denied':'screen_share_unavailable');
      setError(denied?'Screen sharing was denied or cancelled. Share a screen to enable screenshot evidence.':failure instanceof Error?failure.message:'Unable to share screen.');
    } finally{if(alive.current)setBusy(false);}
  },[observations.record,stop]);
  useEffect(()=>{
    async function capture(event:BrowserObservation){
      if(!isActive.current || !stream.current || !video.current || !queue.current)return;
      if(event.kind==='fullscreen_entered'){entered.current=true;entryRecorded.current=true;}
      if(document.fullscreenElement)entered.current=true;
      if(!entered.current || !flags.has(event.kind))return;
      try {
        const preview=video.current;
        if(preview.readyState<2)await new Promise<void>(resolve=>{
          const ready=()=>{clearTimeout(timer);preview.removeEventListener('loadeddata',ready);resolve();};
          const timer=setTimeout(ready,2000);preview.addEventListener('loadeddata',ready,{once:true});
        });
        if(!isActive.current || preview!==video.current || !stream.current)return;
        const captured=Date.now();const blob=await snapshot(preview,1280);
        if(!blob)throw new Error('Screen frame unavailable');
        const selected=stream.current.getVideoTracks()[0]?.getSettings().displaySurface ?? 'unknown';
        await queue.current?.enqueue(event,[{id:crypto.randomUUID(),blob,source:'screen',surface:selected,captured}]);
      }catch{observations.record('evidence_capture_failed');setStatus('Screen snapshot could not be saved. Browser event remains recorded.');}
    }
    const unsubscribe=observations.subscribe(event=>{void capture(event);});
    if(active && shared && document.fullscreenElement && !entryRecorded.current)observations.record('fullscreen_entered');
    return unsubscribe;
  },[active,shared,observations.record,observations.subscribe]);
  return {shared,busy,surface,status,error,request,stop};
}
export type ScreenEvidence=ReturnType<typeof useScreenEvidence>;

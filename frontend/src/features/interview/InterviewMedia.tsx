import { Button } from '../../components/ui/button';
import { evidenceQueue } from './proctoring/evidence';
import { useEffect, useRef, useState } from 'react';
import { startMediaMonitor } from './proctoring/monitor';
import type { InterviewDevices } from './useInterviewDevices';
export function InterviewMedia({token,sessionId,devices}:{token:string;sessionId:string;devices:InterviewDevices}) {
  const video=useRef<HTMLVideoElement>(null);const alive=useRef(true);
  const evidence=useRef<ReturnType<typeof evidenceQueue>|null>(null);
  const automaticRestarts=useRef(0);
  const monitor=useRef<ReturnType<typeof startMediaMonitor>|null>(null);
  const [evidenceStatus,setEvidenceStatus]=useState('');const [monitorStatus,setMonitorStatus]=useState('Media checks waiting for a device');
  const [detection,setDetection]=useState('');
  const [error,setError]=useState('');const [level,setLevel]=useState(0);
  const [monitorAttempt,setMonitorAttempt]=useState(0);
  const camera=devices.camera==='enabled';const mic=devices.microphone==='enabled';
  const cameraTrack=devices.stream.current?.getVideoTracks().find(track=>track.readyState==='live');
  useEffect(()=>{alive.current=true;return()=>{alive.current=false;};},[]);
  useEffect(()=>{
    const preview=video.current;if(!preview)return;
    let cancelled=false;
    setError('');
    const playing=()=>{if(!cancelled)setError('');};
    preview.addEventListener('playing',playing);
    preview.srcObject=camera && cameraTrack?new MediaStream([cameraTrack]):null;
    if(camera && cameraTrack)void preview.play().then(playing).catch(failure=>{
      // Replacing a stream or StrictMode cleanup can reject an older play().
      // Neither is a permission denial, and neither may overwrite the new preview.
      if(cancelled || !alive.current || (failure instanceof DOMException && failure.name==='AbortError'))return;
      setError(failure instanceof DOMException && failure.name==='NotAllowedError'
        ? 'Camera playback was blocked.'
        : 'Camera preview could not start.');
    });
    return()=>{cancelled=true;preview.removeEventListener('playing',playing);preview.srcObject=null;};
    // Microphone revisions must not detach an unchanged camera stream.
  },[camera,cameraTrack]);
  useEffect(()=>{
    if(!mic || !devices.stream.current?.getAudioTracks().length){setLevel(0);return;}
    const context=new AudioContext();const analyser=context.createAnalyser();analyser.fftSize=256;
    context.createMediaStreamSource(new MediaStream(devices.stream.current.getAudioTracks())).connect(analyser);
    const data=new Uint8Array(analyser.fftSize);
    const timer=setInterval(()=>{analyser.getByteTimeDomainData(data);setLevel(Math.min(100,Math.sqrt(data.reduce((sum,value)=>sum+((value-128)/128)**2,0)/data.length)*400));},100);
    void context.resume();return()=>{clearInterval(timer);void context.close();};
  },[mic,devices.revision,devices.stream]);
  useEffect(()=>{const queue=evidenceQueue(token,sessionId,setEvidenceStatus);evidence.current=queue;return()=>{queue.stop();evidence.current=null;};},[token,sessionId]);
  useEffect(()=>{
    setDetection('');
    if((!camera && !mic)||!devices.stream.current||!video.current||!evidence.current){setMonitorStatus('Media checks waiting for a device');return;}
    const next=startMediaMonitor(video.current,devices.stream.current,setMonitorStatus,evidence.current,setDetection,()=>{
      if(automaticRestarts.current>=3){setMonitorStatus('Face/voice checks need attention after 3 recovery attempts. Use Restart face/voice checks.');return;}
      automaticRestarts.current++;setMonitorAttempt(value=>value+1);
    });monitor.current=next;
    return()=>{next.stop();monitor.current=null;};
  },[camera,mic,devices.revision,devices.stream,token,sessionId,monitorAttempt]);
  return <section className="room-media"><div className="camera-preview"><video ref={video} autoPlay muted playsInline hidden={!camera}/>{!camera&&<div><span aria-hidden="true">◎</span><p>Camera {devices.camera}</p></div>}<small>{camera||mic?'Camera/mic enabled':'Browser monitoring continues'}</small></div><div className="media-controls"><Button variant="secondary" aria-pressed={camera} disabled={devices.busy} onClick={()=>void devices.toggle('video')}>{camera?'Turn camera off':'Enable camera'}</Button><Button variant="secondary" aria-pressed={mic} disabled={devices.busy} onClick={()=>void devices.toggle('audio')}>{mic?'Turn mic off':'Enable microphone'}</Button>{mic&&<meter min="0" max="100" value={level} aria-label="Microphone input level"/>}</div>
    <details className="candidate-monitoring">
      <summary>Monitoring details</summary>
      <p>Face, eye/head direction and speech checks run on this device. Flagged screenshots, camera snapshots and short clips are saved for interviewer review. Speech clips may include up to three preceding seconds.</p>
      <p role="status">{monitorStatus}</p>
      {detection && <p>{detection}</p>}
      {evidenceStatus && <p role="status">{evidenceStatus}</p>}
      {(camera || mic) && <div className="candidate-monitoring-actions">
        <Button variant="secondary" onClick={() => {
          automaticRestarts.current = 0;
          setMonitorAttempt(value => value + 1);
        }}>Restart face/voice checks</Button>
        {camera && <Button variant="secondary" onClick={() => monitor.current?.recalibrate()}>Recalibrate eye/head direction</Button>}
      </div>}
    </details>
    {monitorStatus.includes('need attention') && <p className="configuration-note" role="status">Media checks need attention. Open Monitoring details to restart them.</p>}
    {devices.error&&<p className="configuration-note" role="status">{devices.error}</p>}{error&&<p className="error" role="alert">{error}{' '}<Button variant="text" onClick={()=>{void video.current?.play().then(()=>setError('')).catch(()=>setError('Turn the camera off and enable it again.'));}}>Retry camera preview</Button></p>}</section>;
}

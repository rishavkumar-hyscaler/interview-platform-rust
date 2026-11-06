import { evidenceQueue } from './proctoring/evidence';
import { useEffect, useRef, useState } from 'react';
import { startMediaMonitor } from './proctoring/monitor';
import type { InterviewDevices } from './useInterviewDevices';
export function InterviewMedia({token,sessionId,devices}:{token:string;sessionId:string;devices:InterviewDevices}) {
  const video=useRef<HTMLVideoElement>(null);const alive=useRef(true);
  const evidence=useRef<ReturnType<typeof evidenceQueue>|null>(null);
  const monitor=useRef<ReturnType<typeof startMediaMonitor>|null>(null);
  const [evidenceStatus,setEvidenceStatus]=useState('');const [monitorStatus,setMonitorStatus]=useState('Media checks waiting for a device');
  const [detection,setDetection]=useState('');
  const [error,setError]=useState('');const [level,setLevel]=useState(0);
  const camera=devices.camera==='enabled';const mic=devices.microphone==='enabled';
  useEffect(()=>{alive.current=true;return()=>{alive.current=false;};},[]);
  useEffect(()=>{
    const preview=video.current;if(!preview)return;
    preview.srcObject=camera && devices.stream.current?new MediaStream(devices.stream.current.getVideoTracks()):null;
    if(camera)void preview.play().catch(()=>{if(alive.current)setError('Camera playback was blocked. Click Retry camera preview.');});
    return()=>{preview.srcObject=null;};
  },[camera,devices.revision,devices.stream]);
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
    const next=startMediaMonitor(video.current,devices.stream.current,setMonitorStatus,evidence.current,setDetection);monitor.current=next;
    return()=>{next.stop();monitor.current=null;};
  },[camera,mic,devices.revision,devices.stream,token,sessionId]);
  return <section className="room-media"><div className="camera-preview"><video ref={video} autoPlay muted playsInline hidden={!camera}/>{!camera&&<div><span aria-hidden="true">◎</span><p>Camera {devices.camera}</p></div>}<small>{camera||mic?'Camera/mic enabled':'Browser monitoring continues'}</small></div><div className="media-controls"><button className="button secondary" aria-pressed={camera} disabled={devices.busy} onClick={()=>void devices.toggle('video')}>{camera?'Turn camera off':'Enable camera'}</button><button className="button secondary" aria-pressed={mic} disabled={devices.busy} onClick={()=>void devices.toggle('audio')}>{mic?'Turn mic off':'Enable microphone'}</button>{mic&&<meter min="0" max="100" value={level} aria-label="Microphone input level"/>}</div><small className="muted">Face, eye/head direction and speech checks run locally when devices are enabled. Flagged screen/camera snapshots and 3-second clips are saved for interviewer review. Only flagged evidence is uploaded; speech clips include up to 3 preceding seconds held in memory. No identity or emotion analysis.</small><small role="status">{monitorStatus}</small>{detection&&<small role="status">{detection}</small>}{camera&&<button className="text-button" onClick={()=>monitor.current?.recalibrate()}>Recalibrate eye/head direction</button>}{evidenceStatus&&<small role="status">{evidenceStatus}</small>}{devices.error&&<p className="configuration-note" role="status">{devices.error}</p>}{error&&<p className="error" role="alert">{error}<button className="text-button" onClick={()=>{void video.current?.play().then(()=>setError('')).catch(()=>setError('Turn the camera off and enable it again.'));}}>Retry camera preview</button></p>}</section>;
}

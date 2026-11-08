import { ExpandableSection } from './ExpandableSection';
type Observation = { kind: string; character_count?: number | null; measurement?: number | null; unit?: string; measurements?: Record<string, number | null> };
const descriptions: Record<string,string> = {
  camera_permission_requested:'The browser requested camera access.', camera_enabled:'Camera access was enabled.', camera_disabled:'The camera was turned off or its track ended.', camera_denied:'Camera permission was denied.', camera_unavailable:'The camera could not be accessed.',
  microphone_permission_requested:'The browser requested microphone access.', microphone_enabled:'Microphone access was enabled.', microphone_disabled:'The microphone was turned off or its track ended.', microphone_denied:'Microphone permission was denied.', microphone_unavailable:'The microphone could not be accessed.',
  screen_share_requested:'The screen-sharing picker was requested.', screen_share_started:'Screen sharing started.', screen_share_stopped:'Screen sharing stopped.', screen_share_denied:'Screen-sharing permission was denied.', screen_share_unavailable:'Screen sharing was unavailable.',
  fullscreen_entered:'The interview entered fullscreen.', fullscreen_exited:'The interview left fullscreen.', focus_lost:'The interview window lost focus.', focus_returned:'Focus returned to the interview window.', hidden:'The interview tab became hidden.', visible:'The interview tab became visible.',
  monitoring_started:'Local face/voice monitoring started.', monitoring_stopped:'Local face/voice monitoring stopped.', monitoring_capability:'The browser reported its monitoring capability.', face_analysis_ready:'Face analysis received a fresh camera result.', audio_analysis_ready:'Microphone analysis started processing audio.', gaze_calibrated:'Eye/head direction calibration completed.', model_unavailable:'A detection model was unavailable or stopped returning fresh results.', evidence_capture_failed:'An evidence capture or queue operation failed.',
  face_missing:'No face was detected for a sustained period.', multiple_faces:'More than one face was detected for a sustained period.', gaze_away:'Eye/head direction differed from the calibrated baseline for a sustained period.', possible_background_speech:'Speech was detected with face/mouth signals suggesting a possible background source. This does not identify another speaker.', speech_detected:'Speech was detected without fresh camera information.',
  copy:'A copy action was observed. Clipboard text is not stored.', paste:'A paste action was observed. Clipboard text is not stored.', copy_from_question:'Text was copied from the question. This is permitted.', paste_from_question:'Previously copied question text was pasted. This is permitted.', question_context_menu:'The question’s context menu was opened. This is permitted.', context_menu:'A context menu was opened.',
  multiple_displays:'The browser reported a possible additional display.', devtools_suspected:'A browser-window size heuristic suggested developer tools might be open; this is not confirmed.', typing_burst:'A large text change was observed.', rapid_answer:'An answer was submitted shortly after the question opened.', recording_denied:'Answer recording permission was denied.',
};
export function observationDescription(event: Observation) {
  const facts:string[]=[];
  if(event.character_count!=null) facts.push(`${event.character_count} characters`);
  else if(event.measurement!=null && event.unit==='minimum_displays') facts.push(`At least ${event.measurement} displays reported`);
  else if(event.measurement!=null && event.unit==='seconds') facts.push(`${event.measurement} seconds`);
  else if(event.measurement!=null && event.kind==='monitoring_capability') facts.push(event.measurement ? 'Capability available' : 'Capability unavailable');
  const m=event.measurements;
  if(m?.duration_ms!=null)facts.push(`${(m.duration_ms/1000).toFixed(1)} seconds`);
  if(m?.face_count!=null)facts.push(`${m.face_count} faces detected`);
  if(m?.speech_probability!=null)facts.push(`${Math.round(m.speech_probability*100)}% speech confidence`);
  return `${descriptions[event.kind] ?? 'A browser activity observation was recorded.'}${facts.length ? ` ${facts.join(' · ')}.` : ''}`;
}
export function BrowserObservations({events}:{events:{id:string;received_at:string;observation:Observation}[]}) {
  return <ExpandableSection label={<>View observations ({events.length})</>}>{events.map(event=><article className="browser-observation" key={event.id}><strong>{event.observation.kind.replaceAll('_',' ')}</strong><small> · {new Date(event.received_at).toLocaleString()}</small><p>{observationDescription(event.observation)}</p><ExpandableSection label={<>Technical details</>}><code>Event ID: {event.id}</code><pre className="preserve">{JSON.stringify(event.observation,null,2)}</pre></ExpandableSection></article>)}</ExpandableSection>;
}

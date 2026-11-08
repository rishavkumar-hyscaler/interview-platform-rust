import { request } from '../../api';
export type SignalState = { id: string; offer: string | null; answer: string | null; ice_servers: RTCIceServer[] };
export function signal(path: string, token: string, action: string, id?: string, sdp?: string, abort?: AbortSignal) {
  return request<SignalState>(path, { method: 'POST', body: JSON.stringify({ action, id, sdp }), signal: abort }, token);
}
export async function gather(peer: RTCPeerConnection, abort: AbortSignal) {
  if (peer.iceGatheringState === 'complete') return;
  await new Promise<void>((resolve, reject) => {
    const finish = (error?: Error) => {
      clearTimeout(timer); peer.removeEventListener('icegatheringstatechange', change); abort.removeEventListener('abort', cancel);
      if (error) reject(error); else resolve();
    };
    const change = () => { if (peer.iceGatheringState === 'complete') finish(); };
    const cancel = () => finish(new Error('Live view stopped'));
    const timer = setTimeout(() => finish(new Error('Connection setup timed out. Check ICE/TURN configuration.')), 12000);
    peer.addEventListener('icegatheringstatechange', change); abort.addEventListener('abort', cancel, { once: true });
    if (abort.aborted) cancel(); else change();
  });
}
export const hasViewer = (id: string) => id !== '00000000-0000-0000-0000-000000000000';

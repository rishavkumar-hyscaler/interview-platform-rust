import { FaceLandmarker, FilesetResolver as VisionFiles } from '@mediapipe/tasks-vision';
import { AudioClassifier } from '@mediapipe/tasks-audio';
import { faceGeometry } from './signals';
let face: FaceLandmarker | null = null;
let audio: AudioClassifier | null = null;
const scope = globalThis as unknown as { onmessage: (event: MessageEvent) => void; postMessage: (data: unknown) => void };
scope.onmessage = async ({ data }) => {
  try {
    if (data.type === 'init') {
      const base = data.base as string;
      if (data.kind === 'vision') face = await FaceLandmarker.createFromOptions(await VisionFiles.forVisionTasks(`${base}/vision`), { baseOptions: { modelAssetPath: `${base}/face_landmarker.task`, delegate: 'CPU' }, runningMode: 'IMAGE', numFaces: 3, minFaceDetectionConfidence: 0.65, minFacePresenceConfidence: 0.65, outputFaceBlendshapes: false, outputFacialTransformationMatrixes: false });
      else audio = await AudioClassifier.createFromOptions(await VisionFiles.forAudioTasks(`${base}/audio`), { baseOptions: { modelAssetPath: `${base}/yamnet.tflite` }, maxResults: 10, scoreThreshold: 0.1 });
      scope.postMessage({ type: 'ready', kind: data.kind });
    } else if (data.type === 'frame' && face) {
      try { scope.postMessage({ type: 'face', at: data.at, sample: faceGeometry(face.detect(data.frame).faceLandmarks) }); } finally { data.frame.close(); }
    } else if (data.type === 'audio' && audio) {
      const results = audio.classify(data.samples, data.sampleRate);
      const probability = Math.max(0, ...results.flatMap(result => result.classifications.flatMap(head => head.categories.filter(category => ['Speech', 'Conversation', 'Narration, monologue', 'Speech synthesizer'].includes(category.categoryName)).map(category => category.score))));
      scope.postMessage({ type: 'speech', at: data.at, probability });
    }
  } catch { scope.postMessage({ type: 'failure', kind: data.kind ?? (data.type === 'frame' ? 'vision' : 'audio') }); }
};

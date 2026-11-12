/* Mono PCM windows for local speech classification; never connected to audible output. */
class ProctorPCM extends AudioWorkletProcessor {
  constructor() {
    super();
    this.samples = new Float32Array(sampleRate);
    this.offset = 0;
  }
  process(inputs, outputs) {
    outputs[0]?.forEach((channel) => channel.fill(0));
    const channels = inputs[0];
    if (!channels?.length) return true;
    for (let index = 0; index < channels[0].length; index++) {
      this.samples[this.offset++] =
        channels.reduce((sum, channel) => sum + channel[index], 0) /
        channels.length;
      if (this.offset === this.samples.length) {
        this.port.postMessage(this.samples, [this.samples.buffer]);
        this.samples = new Float32Array(sampleRate);
        this.offset = 0;
      }
    }
    return true;
  }
}
registerProcessor('proctor-pcm', ProctorPCM);

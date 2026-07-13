import { decodeMessage } from "@voxelize/core";

interface DecodeRequest {
  buffer: ArrayBuffer;
  generation: number;
  sequence: number;
}

onmessage = (event: MessageEvent<DecodeRequest>) => {
  const { buffer, generation, sequence } = event.data;
  try {
    const transferables: ArrayBuffer[] = [];
    const message = decodeMessage(new Uint8Array(buffer), transferables);
    postMessage({ generation, sequence, message }, { transfer: transferables });
  } catch {
    postMessage({ generation, sequence, error: true });
  }
};

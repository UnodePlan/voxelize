import type { Page } from "@playwright/test";

const SAMPLE_WIDTH = 96;
const SAMPLE_HEIGHT = 54;

export interface CanvasVisualMetrics {
  dominantColorRatio: number;
  edgeRatio: number;
  glErrors: [number, number];
  height: number;
  luminanceStdDev: number;
  nonDarkRatio: number;
  sampleHeight: number;
  sampleWidth: number;
  uniqueColors: number;
  width: number;
}

export interface CanvasVisualSample {
  contextType: "webgl" | "webgl2" | null;
  error: string | null;
  metrics: CanvasVisualMetrics | null;
  rgb: number[];
}

export interface CanvasDifference {
  changedRatio: number;
  meanChannelDelta: number;
  sampledPixels: number;
}

export async function captureCanvasSample(
  page: Page,
  selector: string,
): Promise<CanvasVisualSample> {
  return page.evaluate(
    async ({ sampleHeight, sampleWidth, target }) => {
      const canvas = document.querySelector<HTMLCanvasElement>(target);
      if (canvas === null) {
        return missingSample("live canvas is missing");
      }
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => resolve()),
      );
      const webgl2 = canvas.getContext("webgl2");
      const gl = webgl2 ?? canvas.getContext("webgl");
      if (gl === null) return missingSample("WebGL context is unavailable");
      const contextType = webgl2 === null ? "webgl" : "webgl2";
      if (gl.isContextLost())
        return missingSample("WebGL context is lost", contextType);
      const width = gl.drawingBufferWidth;
      const height = gl.drawingBufferHeight;
      if (width <= 0 || height <= 0) {
        return missingSample(
          `invalid drawing buffer ${width}x${height}`,
          contextType,
        );
      }

      const beforeError = gl.getError();
      const pixels = new Uint8Array(width * height * 4);
      gl.readPixels(0, 0, width, height, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
      const afterError = gl.getError();
      const actualWidth = Math.min(sampleWidth, width);
      const actualHeight = Math.min(sampleHeight, height);
      const rgb: number[] = [];
      const colors = new Map<number, number>();
      const luminance: number[] = [];
      let nonDark = 0;
      for (let row = 0; row < actualHeight; row += 1) {
        const y = Math.min(
          height - 1,
          Math.floor(((row + 0.5) * height) / actualHeight),
        );
        for (let column = 0; column < actualWidth; column += 1) {
          const x = Math.min(
            width - 1,
            Math.floor(((column + 0.5) * width) / actualWidth),
          );
          const index = (y * width + x) * 4;
          const red = pixels[index];
          const green = pixels[index + 1];
          const blue = pixels[index + 2];
          rgb.push(red, green, blue);
          if (Math.max(red, green, blue) > 8) nonDark += 1;
          const quantized = (red >> 3) * 1024 + (green >> 3) * 32 + (blue >> 3);
          colors.set(quantized, (colors.get(quantized) ?? 0) + 1);
          luminance.push(red * 0.2126 + green * 0.7152 + blue * 0.0722);
        }
      }
      const total = actualWidth * actualHeight;
      const mean = luminance.reduce((sum, value) => sum + value, 0) / total;
      const variance =
        luminance.reduce((sum, value) => sum + (value - mean) ** 2, 0) / total;
      const dominant = Math.max(...colors.values());
      let edges = 0;
      let edgePairs = 0;
      const delta = (left: number, right: number): number =>
        Math.abs(rgb[left] - rgb[right]) +
        Math.abs(rgb[left + 1] - rgb[right + 1]) +
        Math.abs(rgb[left + 2] - rgb[right + 2]);
      for (let row = 0; row < actualHeight; row += 1) {
        for (let column = 0; column < actualWidth; column += 1) {
          const index = (row * actualWidth + column) * 3;
          if (column + 1 < actualWidth) {
            if (delta(index, index + 3) > 24) edges += 1;
            edgePairs += 1;
          }
          if (row + 1 < actualHeight) {
            if (delta(index, index + actualWidth * 3) > 24) edges += 1;
            edgePairs += 1;
          }
        }
      }
      return {
        contextType,
        error: null,
        metrics: {
          dominantColorRatio: dominant / total,
          edgeRatio: edges / edgePairs,
          glErrors: [beforeError, afterError] as [number, number],
          height,
          luminanceStdDev: Math.sqrt(variance),
          nonDarkRatio: nonDark / total,
          sampleHeight: actualHeight,
          sampleWidth: actualWidth,
          uniqueColors: colors.size,
          width,
        },
        rgb,
      };

      function missingSample(
        error: string,
        contextType: "webgl" | "webgl2" | null = null,
      ): CanvasVisualSample {
        return { contextType, error, metrics: null, rgb: [] };
      }
    },
    {
      sampleHeight: SAMPLE_HEIGHT,
      sampleWidth: SAMPLE_WIDTH,
      target: selector,
    },
  );
}

export function canvasEvidenceErrors(sample: CanvasVisualSample): string[] {
  if (sample.error !== null) return [sample.error];
  const metrics = sample.metrics;
  if (metrics === null) return ["canvas metrics are missing"];
  const errors: string[] = [];
  if (metrics.glErrors.some((value) => value !== 0))
    errors.push("WebGL reported an error");
  if (metrics.nonDarkRatio < 0.03)
    errors.push("less than 3% of sampled pixels are visible");
  if (metrics.uniqueColors < 4) errors.push("fewer than 4 quantized colors");
  if (metrics.dominantColorRatio > 0.97)
    errors.push("one color occupies more than 97%");
  if (metrics.luminanceStdDev < 4) errors.push("luminance variance is too low");
  if (metrics.edgeRatio < 0.005)
    errors.push("rendered edge density is too low");
  return errors;
}

export function compareCanvasSamples(
  before: CanvasVisualSample,
  after: CanvasVisualSample,
): CanvasDifference {
  const channels = Math.min(before.rgb.length, after.rgb.length);
  if (channels === 0 || channels % 3 !== 0) {
    throw new Error("canvas fingerprints cannot be compared");
  }
  let changed = 0;
  let totalDelta = 0;
  for (let index = 0; index < channels; index += 3) {
    const pixelDelta =
      Math.abs(before.rgb[index] - after.rgb[index]) +
      Math.abs(before.rgb[index + 1] - after.rgb[index + 1]) +
      Math.abs(before.rgb[index + 2] - after.rgb[index + 2]);
    totalDelta += pixelDelta;
    if (pixelDelta > 30) changed += 1;
  }
  const sampledPixels = channels / 3;
  return {
    changedRatio: changed / sampledPixels,
    meanChannelDelta: totalDelta / channels,
    sampledPixels,
  };
}

export function summarizeCanvasSample(sample: CanvasVisualSample): unknown {
  return {
    contextType: sample.contextType,
    error: sample.error,
    metrics: sample.metrics,
  };
}

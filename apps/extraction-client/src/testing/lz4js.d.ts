declare module "lz4js" {
  export function decompress(input: Uint8Array): Uint8Array;
  export function decompressBlock(
    input: Uint8Array,
    output: Uint8Array,
    inputOffset: number,
    inputLength: number,
    outputOffset: number,
  ): number;
}

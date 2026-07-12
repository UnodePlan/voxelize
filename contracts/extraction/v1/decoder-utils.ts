import type { JsonRecord } from "./types";

const MAX_U32 = 4_294_967_295;
const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export function readRecord(value: unknown, path: string): JsonRecord {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${path}: expected object`);
  }
  return value as JsonRecord;
}

export function readArray(value: unknown, path: string): unknown[] {
  if (!Array.isArray(value)) {
    throw new Error(`${path}: expected array`);
  }
  return value;
}

export function readNonEmptyString(value: unknown, path: string): string {
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`${path}: expected non-empty string`);
  }
  return value;
}

export function readUnsignedInteger(value: unknown, path: string): number {
  if (
    !Number.isInteger(value) ||
    (value as number) < 0 ||
    (value as number) > MAX_U32
  ) {
    throw new Error(`${path}: expected unsigned 32-bit integer`);
  }
  return value as number;
}

export function readPositiveInteger(value: unknown, path: string): number {
  const integer = readUnsignedInteger(value, path);
  if (integer === 0) {
    throw new Error(`${path}: expected positive integer`);
  }
  return integer;
}

export function readBoolean(value: unknown, path: string): boolean {
  if (typeof value !== "boolean") {
    throw new Error(`${path}: expected boolean`);
  }
  return value;
}

export function readRequestId(value: unknown): string {
  const requestId = readNonEmptyString(value, "requestId");
  if (!UUID_PATTERN.test(requestId)) {
    throw new Error("requestId: expected UUID");
  }
  return requestId;
}

export function readEnum<const T extends readonly string[]>(
  value: unknown,
  allowed: T,
  path: string,
): T[number] {
  const candidate = readNonEmptyString(value, path);
  if (!(allowed as readonly string[]).includes(candidate)) {
    throw new Error(`${path}: unsupported value ${candidate}`);
  }
  return candidate as T[number];
}

export function assertOnlyKeys(
  source: JsonRecord,
  allowed: string[],
  path: string,
): void {
  for (const key of Object.keys(source)) {
    if (!allowed.includes(key)) {
      throw new Error(`${path}: unknown field ${key}`);
    }
  }
}

export function assertUnique(
  values: Array<string | number>,
  path: string,
): void {
  if (new Set(values).size !== values.length) {
    throw new Error(`${path}: duplicate value`);
  }
}

export function assertExactSet(
  actual: readonly string[],
  expected: readonly string[],
  path: string,
): void {
  assertUnique([...actual], path);
  if (
    actual.length !== expected.length ||
    expected.some((value) => !actual.includes(value))
  ) {
    throw new Error(`${path}: incomplete key set`);
  }
}

const ID_MISMATCH_ERROR =
  "Something went wrong with IDs! Better check if you're passing two same ID's to the same Voxelize server.";

export function resolveInitClientId(
  currentId: string,
  nextId: string,
  allowReplacement: boolean,
): string {
  if (!nextId) {
    return currentId;
  }
  if (currentId && currentId !== nextId && !allowReplacement) {
    throw new Error(ID_MISMATCH_ERROR);
  }
  return nextId;
}

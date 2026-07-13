import { readdir, readFile, stat } from "node:fs/promises";
import { resolve } from "node:path";

const forbidden = ["__VOXEL_EXTRACTION_E2E__", "e2e-main", "core-facade"];
const files = await collectFiles(resolve("dist"));

for (const file of files) {
  if (!/\.(?:html|js|css)$/u.test(file)) continue;
  const content = await readFile(file, "utf8");
  const token = forbidden.find((candidate) => content.includes(candidate));
  if (token !== undefined) {
    throw new Error(`production bundle contains test bridge token ${token}`);
  }
}

async function collectFiles(directory) {
  const entries = await readdir(directory);
  const nested = await Promise.all(
    entries.map(async (entry) => {
      const path = resolve(directory, entry);
      return (await stat(path)).isDirectory() ? collectFiles(path) : [path];
    }),
  );
  return nested.flat();
}

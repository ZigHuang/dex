import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, beforeEach } from "@jest/globals";

const originalDexHome = process.env.DEX_HOME;
let currentDexHome: string | undefined;

beforeEach(async () => {
  currentDexHome = await fs.mkdtemp(path.join(os.tmpdir(), "dex-sdk-test-"));
  process.env.DEX_HOME = currentDexHome;
});

afterEach(async () => {
  const dexHomeToDelete = currentDexHome;
  currentDexHome = undefined;

  if (originalDexHome === undefined) {
    delete process.env.DEX_HOME;
  } else {
    process.env.DEX_HOME = originalDexHome;
  }

  if (dexHomeToDelete) {
    await fs.rm(dexHomeToDelete, { recursive: true, force: true });
  }
});

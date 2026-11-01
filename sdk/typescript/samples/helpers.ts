import path from "node:path";

export function dexPathOverride() {
  return (
    process.env.DEX_EXECUTABLE ??
    path.join(process.cwd(), "..", "..", "dex-rs", "target", "debug", "dex")
  );
}

export type DexConfigValue = string | number | boolean | DexConfigValue[] | DexConfigObject;

export type DexConfigObject = { [key: string]: DexConfigValue };

export type DexOptions = {
  dexPathOverride?: string;
  baseUrl?: string;
  apiKey?: string;
  /**
   * Additional `--config key=value` overrides to pass to the Dex CLI.
   *
   * Provide a JSON object and the SDK will flatten it into dotted paths and
   * serialize values as TOML literals so they are compatible with the CLI's
   * `--config` parsing.
   */
  config?: DexConfigObject;
  /**
   * Raw `--config key=value` overrides to pass unchanged to the Dex CLI after
   * structured configuration and before SDK-managed or thread-specific overrides.
   */
  configOverrides?: string[];
  /**
   * Environment variables passed to the Dex CLI process. When provided, the SDK
   * will not inherit variables from `process.env`.
   */
  env?: Record<string, string>;
};

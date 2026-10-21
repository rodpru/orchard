import { message } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { resolveLanguage } from "../i18n";
import { api, errorMessage } from "../lib/api";
import type { Config } from "../lib/types";

/**
 * Loads the config once and applies updates as functions of the latest value,
 * so concurrent updates (a pin toggle during startup registration) never
 * overwrite each other. Saves are written in order, one at a time.
 */
export function useConfig() {
  const { t, i18n } = useTranslation();
  const [config, setConfig] = useState<Config | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const current = useRef<Config | null>(null);
  const saving = useRef<Promise<void>>(Promise.resolve());

  useEffect(() => {
    let cancelled = false;
    api
      .configLoad()
      .then(({ config: loaded, corruptBackup }) => {
        if (cancelled) return;
        current.current = loaded;
        setConfig(loaded);
        if (corruptBackup) {
          void message(t("errors.configCorrupt", { path: corruptBackup }), { kind: "warning" });
        }
      })
      .catch((e: unknown) => !cancelled && setLoadError(errorMessage(e)));
    return () => {
      cancelled = true;
    };
  }, [t]);

  const language = config?.language ?? null;
  useEffect(() => {
    void i18n.changeLanguage(resolveLanguage(language));
  }, [i18n, language]);

  const updateConfig = useCallback((update: (config: Config) => Config): Promise<void> => {
    const previous = current.current;
    if (!previous) return Promise.resolve();
    const next = update(previous);
    current.current = next;
    setConfig(next);
    saving.current = saving.current.catch(() => undefined).then(() => api.configSave(next));
    return saving.current;
  }, []);

  return { config, loadError, updateConfig };
}

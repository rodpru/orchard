import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import en from "./locales/en.json";
import pt from "./locales/pt.json";
import type { Language } from "../lib/types";

export const LANGUAGES: readonly Language[] = ["en", "pt"];

export function resolveLanguage(preference: Language | null): Language {
  if (preference) return preference;
  return navigator.language.toLowerCase().startsWith("pt") ? "pt" : "en";
}

void i18n.use(initReactI18next).init({
  resources: { en: { translation: en }, pt: { translation: pt } },
  lng: resolveLanguage(null),
  fallbackLng: "en",
  interpolation: { escapeValue: false },
});

export default i18n;

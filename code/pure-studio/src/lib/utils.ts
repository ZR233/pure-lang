import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";
import type { ProviderRecord } from "../types";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function initials(name: string) {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? "")
    .join("");
}

export function errorText(error: unknown) {
  if (typeof error === "object" && error !== null && "message" in error) {
    return String((error as { message: unknown }).message);
  }
  return String(error);
}

export function formatTime(value: number) {
  if (!value) {
    return "";
  }
  return new Date(value * 1000).toLocaleString();
}

export function allModels(provider: ProviderRecord) {
  if (provider.models.length > 0) {
    return provider.models;
  }
  return [...provider.defaultModels, ...provider.customModels];
}

export function providerStatusClass(provider: ProviderRecord) {
  return provider.status.toLowerCase().includes("healthy")
    ? "text-emerald-600"
    : "text-amber-600";
}

const STATUS_KEY_MAP: Record<string, string> = {
  Healthy: "provider.healthy",
  "Needs setup": "provider.needsSetup",
};

export function translateStatus(status: string, t: (key: string) => string) {
  return t(STATUS_KEY_MAP[status] ?? status);
}

const UPDATED_AT_KEY_MAP: Record<string, string> = {
  Draft: "provider.draft",
  Loaded: "provider.loaded",
  Preview: "provider.draft",
};

export function translateUpdatedAt(value: string, t: (key: string) => string) {
  return UPDATED_AT_KEY_MAP[value] ? t(UPDATED_AT_KEY_MAP[value]) : value;
}

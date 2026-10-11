import anthropic from "./anthropic.svg";
import google from "./google.svg";
import openai from "./openai.svg";
import opencode from "./opencode.svg";

export const PROVIDERS = ["anthropic", "google", "openai", "opencode"] as const;

export type Provider = (typeof PROVIDERS)[number];

export const PROVIDER_MARKS: Record<Provider, string> = { anthropic, google, openai, opencode };

import { PROVIDERS, type ProviderKind } from '@hearth/shared';
import type { TranslateFn } from '@/i18n/index.ts';

/**
 * Display name of a provider kind. Brand names stay as they are; the generic
 * "OpenAI-compatible" kind is a description and therefore translated.
 *
 * @param t - Translation function.
 * @param kind - Provider kind.
 * @returns The name to show in the UI.
 */
export function providerName(t: TranslateFn, kind: ProviderKind): string {
  return kind === 'openai-compatible' ? t('providers.kindOpenAICompatible') : PROVIDERS[kind].label;
}

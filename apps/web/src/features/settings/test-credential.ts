import type { ProviderCredential } from '@harness/shared';
import { toast } from 'sonner';
import type { TranslateFn } from '@/i18n/index.ts';
import { api, errorMessage } from '@/lib/api.ts';

/**
 * Tests a credential against its provider and reports the result as a toast.
 *
 * @param t - Translation function.
 * @param credential - Credential to test.
 * @returns Resolves when the toast was shown; never rejects.
 */
export async function testCredential(t: TranslateFn, credential: ProviderCredential): Promise<void> {
  try {
    const result = await api.providers.test(credential.id);
    if (result.error) {
      toast.error(t('providers.testFailed'), { description: result.error });
    } else {
      toast.success(t('providers.testOk', { count: result.models.length }), { description: credential.label });
    }
  } catch (error) {
    toast.error(t('providers.testFailed'), { description: errorMessage(t, error) });
  }
}

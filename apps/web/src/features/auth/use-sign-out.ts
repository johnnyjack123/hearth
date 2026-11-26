import { useQueryClient } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { authClient } from '@/lib/auth-client.ts';
import { clearDrafts } from '@/lib/drafts.ts';
import { clearOfflineData } from '@/lib/offline.ts';
import { disposeAllChats } from '../chat/chat-store.ts';

/**
 * Signs out, drops all cached data and running chats of the account, and
 * returns to the login page.
 *
 * @returns A function that performs the sign-out.
 */
export function useSignOut(): () => Promise<void> {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  return async () => {
    await authClient.signOut();
    disposeAllChats();
    // Unsent text and the chats kept for offline reading stay out of the
    // next person's reach.
    clearDrafts();
    await clearOfflineData();
    queryClient.clear();
    await navigate({ to: '/login' });
  };
}

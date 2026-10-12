import { Server, Users } from 'lucide-react';
import { useState } from 'react';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs.tsx';
import { useT } from '@/lib/preferences.tsx';
import { Page } from '../shell/page.tsx';
import { ServerPanel } from './server-panel.tsx';
import { UsersPanel } from './users-panel.tsx';

/**
 * Administration page (`/admin`, admins only): user management and server settings.
 *
 * @returns The admin page.
 */
export function AdminPage() {
  const t = useT();
  const [tab, setTab] = useState('users');
  return (
    <Page title={t('admin.title')}>
      <Tabs value={tab} onValueChange={setTab}>
        <TabsList className="mb-8">
          <TabsTrigger value="users">
            <Users />
            {t('admin.tabUsers')}
          </TabsTrigger>
          <TabsTrigger value="server">
            <Server />
            {t('admin.tabServer')}
          </TabsTrigger>
        </TabsList>
        <TabsContent value="users">
          <UsersPanel />
        </TabsContent>
        <TabsContent value="server">
          <ServerPanel />
        </TabsContent>
      </Tabs>
    </Page>
  );
}
